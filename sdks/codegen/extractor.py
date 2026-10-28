"""Deterministic extraction of the SDK contract projection.

Selects the supported application operations from the OAuth API contract,
collects their transitive local component references, and strips
authentication and AWS deployment integration so the SDK carries only the
wire shape.
"""

import yaml

SUPPORTED_OPERATION_IDS = (
    "PublishEvent",
    "Subscribe",
    "Unsubscribe",
    "DescribeSubscription",
)

HTTP_METHODS = ("get", "put", "post", "delete", "options", "head", "patch", "trace")

STRIPPED_KEYS = frozenset({"security", "servers"})
STRIPPED_PREFIXES = ("x-amazon-apigateway",)


class ExtractionError(Exception):
    """The maintained contracts cannot produce a trustworthy SDK projection."""


def iter_operations(doc):
    for path, item in doc.get("paths", {}).items():
        for method, operation in item.items():
            if method in HTTP_METHODS and isinstance(operation, dict):
                yield path, method, operation


def _select_operations(doc):
    selected = {}
    for path, method, operation in iter_operations(doc):
        operation_id = operation.get("operationId")
        if operation_id in SUPPORTED_OPERATION_IDS:
            selected[(path, method)] = operation
    missing = [
        operation_id
        for operation_id in SUPPORTED_OPERATION_IDS
        if operation_id
        not in {op.get("operationId") for op in selected.values()}
    ]
    if missing:
        raise ExtractionError(
            f"supported operation(s) missing from contract: {', '.join(missing)}"
        )
    return selected


def _local_refs(node, found):
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                if not value.startswith("#/components/"):
                    raise ExtractionError(f"unsupported non-local reference {value}")
                found.add(value)
            else:
                _local_refs(value, found)
    elif isinstance(node, list):
        for value in node:
            _local_refs(value, found)
    return found


def _resolve(doc, ref):
    node = doc
    for part in ref[2:].split("/"):
        if not isinstance(node, dict) or part not in node:
            raise ExtractionError(f"dangling reference {ref}")
        node = node[part]
    return node


def _component_closure(doc, roots):
    seen = set()
    stack = sorted(roots)
    while stack:
        ref = stack.pop()
        if ref in seen:
            continue
        seen.add(ref)
        for nested in _local_refs(_resolve(doc, ref), set()):
            if nested not in seen:
                stack.append(nested)
    return seen


def extract_projection(doc):
    """Project one maintained surface down to the supported operations."""
    selected = _select_operations(doc)
    paths = {}
    for (path, method), operation in selected.items():
        paths.setdefault(path, {})[method] = operation

    refs = _component_closure(doc, _local_refs(paths, set()))
    components = {}
    for ref in sorted(refs):
        group, name = ref[2:].split("/")[1:3]
        components.setdefault(group, {})[name] = _resolve(doc, ref)

    projection = {
        "openapi": doc["openapi"],
        "info": {"title": doc["info"]["title"], "version": doc["info"]["version"]},
        "paths": paths,
        "components": components,
    }
    _apply_output_normalizations(projection)
    return projection


def _schema_names_from(node):
    return {
        ref.rsplit("/", 1)[1]
        for ref in _local_refs(node, set())
        if ref.startswith("#/components/schemas/")
    }


def _drop_status_enums(node):
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "status" and isinstance(value, dict) and "enum" in value:
                value = {k: v for k, v in value.items() if k != "enum"}
                node[key] = value
            _drop_status_enums(value)
    elif isinstance(node, list):
        for value in node:
            _drop_status_enums(value)


def _expand_schema_closure(projection, roots):
    schemas = projection["components"]["schemas"]
    seen = set(roots)
    stack = list(roots)
    while stack:
        schema = schemas.get(stack.pop())
        if schema is None:
            continue
        for nested in _schema_names_from(schema):
            if nested not in seen:
                seen.add(nested)
                stack.append(nested)
    return seen


def _apply_output_normalizations(projection):
    """response-status-enums-as-strings: response status fields are plain
    strings so unfamiliar statuses decode instead of failing enum checks.
    Request-side schemas are never touched."""
    request_names = set()
    response_names = set()
    for _, _, operation in iter_operations(projection):
        request_names |= _schema_names_from(operation.get("requestBody", {}))
        response_names |= _schema_names_from(operation.get("responses", {}))
    response_only = _expand_schema_closure(
        projection, response_names
    ) - _expand_schema_closure(projection, request_names)
    for name in sorted(response_only):
        schema = projection["components"]["schemas"].get(name)
        if schema is not None:
            _drop_status_enums(schema)


def _strip_deployment(node):
    if isinstance(node, dict):
        return {
            key: _strip_deployment(value)
            for key, value in node.items()
            if key not in STRIPPED_KEYS
            and not any(key.startswith(p) for p in STRIPPED_PREFIXES)
        }
    if isinstance(node, list):
        return [_strip_deployment(value) for value in node]
    return node


def build_sdk_contract(oauth_doc):
    """Build the SDK contract from the OAuth (Cognito-authorized) surface.

    Authentication is supplied by the handwritten client, so security
    requirements and API Gateway integration never reach the generator.
    """
    projection = extract_projection(oauth_doc)
    return {
        "openapi": projection["openapi"],
        "info": projection["info"],
        "paths": _strip_deployment(projection["paths"]),
        "components": _strip_deployment(projection["components"]),
    }


def dump_projection(projection):
    return yaml.safe_dump(projection, sort_keys=True, allow_unicode=True)
