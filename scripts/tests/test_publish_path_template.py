#!/usr/bin/env python3
"""Structural checks for the publish-path resources in konductor-core-stack.yaml.

Run: `uv run --with pytest --with pyyaml pytest scripts/tests/test_publish_path_template.py`
(or `python3 scripts/tests/test_publish_path_template.py` with pyyaml installed).

Parses the full template with a CloudFormation-tag-tolerant YAML loader and
asserts the publish-path tuning surface: throttle parameters and their use in
stage method settings, the conditional admission concurrency cap, bounded
delivery concurrency, the throttle-recovery receive budget, liveness-cache and
queue wiring, latency metric filters, and the publish DLQ/queue-age alarms.
"""
import unittest
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parent.parent.parent
TEMPLATE = REPO / "konductor-core-stack.yaml"


class CfnLoader(yaml.SafeLoader):
    """SafeLoader that maps !Ref/!Sub/!GetAtt/... tags to {Name: value}."""


def _cfn_tag(loader, suffix, node):
    if isinstance(node, yaml.ScalarNode):
        value = loader.construct_scalar(node)
        if suffix == "GetAtt" and "." in value:
            return {"Fn::GetAtt": value.split(".", 1)}
        return {suffix: value}
    if isinstance(node, yaml.SequenceNode):
        return {suffix: loader.construct_sequence(node, deep=True)}
    return {suffix: loader.construct_mapping(node, deep=True)}


CfnLoader.add_multi_constructor("!", _cfn_tag)


def load_template():
    return yaml.load(TEMPLATE.read_text(), Loader=CfnLoader)


def getatt(logical_id, attribute):
    return {"Fn::GetAtt": [logical_id, attribute]}


class PublishPathTemplateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = load_template()
        cls.parameters = cls.doc["Parameters"]
        cls.resources = cls.doc["Resources"]
        cls.conditions = cls.doc.get("Conditions", {})

    def parameter(self, name):
        self.assertIn(name, self.parameters, f"parameter {name} missing")
        return self.parameters[name]

    def test_publish_throttle_rate_limit_parameter(self):
        param = self.parameter("PublishThrottleRateLimit")
        self.assertEqual(param["Type"], "Number")
        self.assertEqual(param["Default"], 2000)
        self.assertEqual(param["MinValue"], 1)

    def test_publish_throttle_burst_limit_parameter(self):
        param = self.parameter("PublishThrottleBurstLimit")
        self.assertEqual(param["Type"], "Number")
        self.assertEqual(param["Default"], 1000)
        self.assertEqual(param["MinValue"], 0)

    def test_publish_admission_concurrency_cap_parameter(self):
        param = self.parameter("PublishAdmissionConcurrencyCap")
        self.assertEqual(param["Type"], "Number")
        self.assertEqual(param["Default"], 0)
        self.assertEqual(param["MinValue"], 0)

    def test_publish_delivery_max_concurrency_parameter(self):
        param = self.parameter("PublishDeliveryMaxConcurrency")
        self.assertEqual(param["Type"], "Number")
        self.assertEqual(param["Default"], 50)
        self.assertEqual(param["MinValue"], 2)
        self.assertEqual(param["MaxValue"], 1000)

    def test_domain_liveness_cache_seconds_parameter(self):
        param = self.parameter("DomainLivenessCacheSeconds")
        self.assertEqual(param["Type"], "Number")
        self.assertEqual(param["Default"], 300)
        self.assertEqual(param["MinValue"], 0)
        self.assertEqual(param["MaxValue"], 900)

    def test_authoring_stages_keep_control_plane_throttle_and_add_publish_throttle(self):
        for api in ("KonductorOAuthApi", "KonductorIamApi"):
            with self.subTest(api=api):
                settings = self.resources[api]["Properties"]["MethodSettings"]
                by_path = {(s["ResourcePath"], s["HttpMethod"]): s for s in settings}
                wildcard = by_path.get(("/*", "*"))
                self.assertIsNotNone(wildcard, "stage-wide /* throttle must stay")
                self.assertEqual(wildcard["ThrottlingRateLimit"], 50)
                self.assertEqual(wildcard["ThrottlingBurstLimit"], 100)
                publish = by_path.get(("/~1publish", "POST"))
                self.assertIsNotNone(publish, "publish method needs its own throttle")
                self.assertEqual(
                    publish["ThrottlingRateLimit"], {"Ref": "PublishThrottleRateLimit"}
                )
                self.assertEqual(
                    publish["ThrottlingBurstLimit"], {"Ref": "PublishThrottleBurstLimit"}
                )

    def test_admission_concurrency_cap_condition(self):
        self.assertEqual(
            self.conditions.get("HasPublishAdmissionConcurrencyCap"),
            {"Not": [{"Equals": [{"Ref": "PublishAdmissionConcurrencyCap"}, 0]}]},
        )

    def test_admission_reserved_concurrency_is_conditional(self):
        props = self.resources["PublishEventsAdmissionFunction"]["Properties"]
        self.assertEqual(
            props["ReservedConcurrentExecutions"],
            {
                "If": [
                    "HasPublishAdmissionConcurrencyCap",
                    {"Ref": "PublishAdmissionConcurrencyCap"},
                    {"Ref": "AWS::NoValue"},
                ]
            },
        )

    def test_delivery_scaling_config_bounds_concurrency(self):
        trigger = self.resources["PublishEventsFunction"]["Properties"]["Events"]["SqsTrigger"]["Properties"]
        self.assertEqual(trigger["BatchSize"], 10)
        self.assertEqual(
            trigger["ScalingConfig"]["MaximumConcurrency"],
            {"Ref": "PublishDeliveryMaxConcurrency"},
        )

    def test_publish_queue_throttle_recovery_budget(self):
        props = self.resources["PublishQueue"]["Properties"]
        self.assertEqual(props["VisibilityTimeout"], 30)
        self.assertEqual(props["RedrivePolicy"]["maxReceiveCount"], 25)
        self.assertEqual(
            props["RedrivePolicy"]["deadLetterTargetArn"], getatt("PublishDlq", "Arn")
        )

    def statements(self, function):
        for policy in self.resources[function]["Properties"]["Policies"]:
            if isinstance(policy, dict) and "Statement" in policy:
                yield from policy["Statement"]

    def test_delivery_can_change_message_visibility_on_publish_queue_only(self):
        delivery = list(self.statements("PublishEventsFunction"))
        matching = [
            s for s in delivery
            if "sqs:ChangeMessageVisibility" in s.get("Action", [])
        ]
        self.assertEqual(len(matching), 1)
        self.assertEqual(matching[0]["Resource"], getatt("PublishQueue", "Arn"))

    def test_admission_can_read_publish_queue_attributes(self):
        admission = list(self.statements("PublishEventsAdmissionFunction"))
        matching = [
            s for s in admission
            if "sqs:GetQueueAttributes" in s.get("Action", [])
        ]
        self.assertEqual(len(matching), 1)
        self.assertEqual(matching[0]["Resource"], getatt("PublishQueue", "Arn"))

    def test_delivery_environment(self):
        variables = (
            self.resources["PublishEventsFunction"]["Properties"]
            ["Environment"]["Variables"]
        )
        self.assertEqual(variables["PUBLISH_QUEUE_URL"], {"Ref": "PublishQueue"})
        self.assertEqual(
            variables["DOMAIN_LIVENESS_CACHE_SECONDS"],
            {"Ref": "DomainLivenessCacheSeconds"},
        )

    def test_admission_environment(self):
        variables = (
            self.resources["PublishEventsAdmissionFunction"]["Properties"]
            ["Environment"]["Variables"]
        )
        self.assertEqual(
            variables["DOMAIN_LIVENESS_CACHE_SECONDS"],
            {"Ref": "DomainLivenessCacheSeconds"},
        )

    def metric_filters(self):
        return {
            name: resource["Properties"]
            for name, resource in self.resources.items()
            if resource.get("Type") == "AWS::Logs::MetricFilter"
        }

    def test_latency_metric_filters(self):
        filters = {
            props["MetricTransformations"][0]["MetricName"]: props
            for props in self.metric_filters().values()
        }
        expected = {
            "PublishAdmissionLatency": (
                {"Ref": "PublishEventsAdmissionLogGroup"},
                '{ $.fields.metric = "AdmissionAccepted" }',
                "$.fields.latency_ms",
            ),
            "PublishQueueWait": (
                {"Ref": "PublishEventsLogGroup"},
                '{ $.fields.metric = "PublishDelivered" }',
                "$.fields.queue_wait_ms",
            ),
            "PublishEnqueueToDelivery": (
                {"Ref": "PublishEventsLogGroup"},
                '{ $.fields.metric = "PublishDelivered" }',
                "$.fields.enqueue_to_delivery_ms",
            ),
        }
        for metric, (log_group, pattern, value) in expected.items():
            with self.subTest(metric=metric):
                self.assertIn(metric, filters, f"no metric filter emits {metric}")
                props = filters[metric]
                self.assertEqual(props["LogGroupName"], log_group)
                self.assertEqual(props["FilterPattern"], pattern)
                transformation = props["MetricTransformations"][0]
                self.assertEqual(transformation["MetricNamespace"], "Konductor/EventStreams")
                self.assertEqual(transformation["MetricValue"], value)

    def test_retry_metric_filter_counts_scheduled_retries(self):
        filters = self.metric_filters()
        props = filters.get("PublishDeliveryRetryFilter")
        self.assertIsNotNone(props, "no metric filter counts scheduled publish delivery retries")
        self.assertEqual(props["LogGroupName"], {"Ref": "PublishEventsLogGroup"})
        self.assertEqual(
            props["FilterPattern"],
            '{ $.fields.metric = "PublishDeliveryRetry" && $.fields.message_id = "*" }',
            "only per-message retry logs count; batch-level diagnostics carry no message_id",
        )
        transformation = props["MetricTransformations"][0]
        self.assertEqual(transformation["MetricNamespace"], "Konductor/EventStreams")
        self.assertEqual(transformation["MetricName"], "PublishDeliveryRetry")
        self.assertEqual(transformation["MetricValue"], "1")

    def test_publish_dlq_depth_alarm(self):
        props = self.resources["PublishDlqDepthAlarm"]["Properties"]
        self.assertEqual(props["Namespace"], "AWS/SQS")
        self.assertEqual(props["MetricName"], "ApproximateNumberOfMessagesVisible")
        self.assertEqual(
            props["Dimensions"],
            [{"Name": "QueueName", "Value": getatt("PublishDlq", "QueueName")}],
        )
        self.assertEqual(props["Threshold"], 1)
        self.assertEqual(props["ComparisonOperator"], "GreaterThanOrEqualToThreshold")

    def test_publish_delivery_retry_alarm(self):
        props = self.resources["PublishDeliveryRetryAlarm"]["Properties"]
        self.assertEqual(props["Namespace"], "Konductor/EventStreams")
        self.assertEqual(props["MetricName"], "PublishDeliveryRetry")
        self.assertNotIn("Dimensions", props)
        self.assertEqual(props["Statistic"], "Sum")
        self.assertEqual(props["Period"], 300)
        self.assertEqual(props["EvaluationPeriods"], 2)
        self.assertEqual(props["Threshold"], 1)
        self.assertEqual(props["ComparisonOperator"], "GreaterThanOrEqualToThreshold")
        self.assertEqual(props["TreatMissingData"], "notBreaching")

    def test_publish_queue_age_alarm(self):
        props = self.resources["PublishQueueAgeAlarm"]["Properties"]
        self.assertEqual(props["Namespace"], "AWS/SQS")
        self.assertEqual(props["MetricName"], "ApproximateAgeOfOldestMessage")
        self.assertEqual(
            props["Dimensions"],
            [{"Name": "QueueName", "Value": getatt("PublishQueue", "QueueName")}],
        )
        self.assertEqual(props["Statistic"], "Maximum")
        self.assertEqual(props["Period"], 300)
        self.assertEqual(props["Threshold"], 300)
        self.assertEqual(props["ComparisonOperator"], "GreaterThanOrEqualToThreshold")


if __name__ == "__main__":
    unittest.main()
