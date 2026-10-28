import math

import pytest

import konductor
from konductor import KonductorClient

ENDPOINT = "https://api.example.invalid/prod"
TOKEN = "tok-config"


def test_a_token_source_is_required():
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT)


def test_token_and_provider_are_mutually_exclusive():
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token=TOKEN, token_provider=lambda: TOKEN)


@pytest.mark.parametrize("bad", ["", 123, b"tok"])
def test_fixed_token_must_be_a_nonempty_string(bad):
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token=bad)


def test_token_provider_must_be_callable():
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token_provider="not-callable")


def test_iam_signing_is_not_offered():
    assert not hasattr(konductor, "IamAuth")
    with pytest.raises(TypeError):
        KonductorClient(endpoint=ENDPOINT, auth=object())


@pytest.mark.parametrize(
    "endpoint",
    [
        "http://api.example.invalid/prod",
        "https://user:secret@api.example.invalid/prod",
        "https://api.example.invalid/prod?x=1",
        "https://api.example.invalid/prod#frag",
        "not-a-url",
        "",
    ],
)
def test_unsafe_endpoints_are_rejected(endpoint):
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=endpoint, token=TOKEN)


def test_base_path_is_preserved_with_or_without_trailing_slash():
    for endpoint in (ENDPOINT, ENDPOINT + "/"):
        client = KonductorClient(endpoint=endpoint, token=TOKEN)
        assert client.endpoint == ENDPOINT
        client.close()


@pytest.mark.parametrize(
    "bad",
    [
        0,
        -1,
        float("inf"),
        math.nan,
        pytest.param(10**400, id="overflow"),
        "5",
        None,
        True,
    ],
)
def test_timeouts_must_be_positive_finite_numbers(bad):
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token=TOKEN, connect_timeout=bad)
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token=TOKEN, read_timeout=bad)


def test_valid_custom_timeouts_are_kept():
    client = KonductorClient(
        endpoint=ENDPOINT, token=TOKEN, connect_timeout=1, read_timeout=2.5
    )
    assert client.connect_timeout == 1.0
    assert client.read_timeout == 2.5
    client.close()


def test_invalid_configuration_never_builds_a_transport():
    built = []

    def factory(**kwargs):
        built.append(kwargs)
        raise AssertionError("must not be called")

    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(
            endpoint="http://insecure.invalid", token=TOKEN, transport_factory=factory
        )
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(endpoint=ENDPOINT, token="", transport_factory=factory)
    assert built == []


def test_context_manager_releases_connections():
    closed = []

    class FakeTransport:
        def close(self):
            closed.append(True)

    client = KonductorClient(
        endpoint=ENDPOINT,
        token=TOKEN,
        transport_factory=lambda **kwargs: FakeTransport(),
    )
    with client as entered:
        assert entered is client
        client._transport_client()
    assert closed == [True]


def test_close_is_idempotent_and_client_rejects_use_after_close():
    client = KonductorClient(endpoint=ENDPOINT, token=TOKEN)
    client.close()
    client.close()
    with pytest.raises(konductor.ConfigurationError):
        client._transport_client()


def test_platform_operations_are_not_exposed():
    client = KonductorClient(endpoint=ENDPOINT, token=TOKEN)
    for excluded in (
        "register_domain",
        "list_domains",
        "create_schema",
        "promote_schema",
    ):
        assert not hasattr(client, excluded)
    client.close()


@pytest.mark.parametrize(
    "endpoint",
    ["https://[broken", "https://api.example.invalid:99999/prod", "https://[::1"],
)
def test_unparseable_endpoint_is_a_configuration_error(endpoint):
    built = []
    with pytest.raises(konductor.ConfigurationError):
        KonductorClient(
            endpoint=endpoint,
            token=TOKEN,
            transport_factory=lambda **kwargs: built.append(kwargs),
        )
    assert built == []
