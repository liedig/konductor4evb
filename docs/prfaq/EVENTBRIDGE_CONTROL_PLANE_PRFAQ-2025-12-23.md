# EventBridge \[Control Plane] PR/FAQ

## Press Release

**AWS launches EventBridge \[Control Plane], eliminating cross-account complexity for developers building event-driven architectures**

*Developers can now subscribe to events and start processing them in minutes, without managing event buses, IAM policies, or cross-account delivery infrastructure*

**SEATTLE – March 15, 2026** – Today, Amazon Web Services (AWS) announced the EventBridge \[Control Plane], a new service that dramatically simplifies how developers build Event-Driven Architectures (EDA) using Amazon EventBridge. AWS customers building event-driven applications can now subscribe to events from other domains and start processing them within minutes, eliminating the need to manage complex cross-account event delivery infrastructure that previously required days of setup.

Previously, developers building event-driven applications faced significant technical hurdles when consuming events across AWS accounts. To consume a single event from a central event bus in another account, developers had to create a minimum of two rules - one to route the event from the central bus to their account's event bus, and another to process the event within their account. This process required managing IAM policies on both the central event bus and consumer event bus, understanding cross-account delivery mechanisms, and manually configuring resource policies. For applications consuming hundreds or thousands of events, this complexity became overwhelming. "We don't have a team big enough, or the skills to manage all these resources, we just want AWS to do that," said David, Cloud Architect from Flight Centre.

The EventBridge \[Control Plane] transforms this experience by providing domain abstraction that hides the underlying routing infrastructure from developers. Platform teams can register domains through the \[\[control plane]], which automatically provisions all necessary resources including domain-specific event buses, IAM permissions, schema registries, dead letter queues, and CloudWatch monitoring. Development teams can then use simple "publish", "subscribe", and "unsubscribe" API operations to produce and consume events from any domain they have permission to access. The control plane handles all cross-account delivery complexity behind the scenes, allowing developers to focus entirely on processing the events that matter to their applications.

The \[control plane] also provides unprecedented visibility into event flows across organizations by establishing and tracking connections between domains, showing exactly where events originate and who consumes them. This eliminates the current challenge of understanding complex event relationships across distributed systems. Additionally, the service addresses one of the most difficult aspects of enterprise EDA implementation: schema versioning management. The control plane helps customers manage schema versions, evaluate the impact of schema updates on event consumers, and provide automated migration plans to get all consumers using the latest schema versions.

"Before, I had to spend 1-2 days managing access to event buses, IAM policies and cross account delivery, and define rules for subscriptions and application integration, just to consume an event. Now, all I need to do is subscribe to an event and the cross account delivery is invisible to me. In minutes I can start processing events for my application," said a Flight Centre developer who tested the new service.

The \[control plane] also addresses EventBridge's 2000 rule limit per event bus by automatically distributing events across domain-specific buses while maintaining a unified developer experience. Developers no longer need to know which specific event bus contains the events they want to consume - they simply browse available events in the schema registry and subscribe to what their application needs. This separation of concerns enables platform teams to manage the vast majority of EDA integration complexity while giving development teams the freedom to create applications without depending on platform team support for each new event subscription.

"We designed the EventBridge \[Control Plane] to eliminate the infrastructure burden that was preventing our customers from adopting event-driven architectures at scale," said Nishant Mehta, VP of Amazon EventBridge. "By abstracting away cross-account complexity and providing simple subscription APIs, we enable developers to focus on building great applications rather than managing routing infrastructure."

The EventBridge \[Control Plane] is available today in all AWS regions where Amazon EventBridge is supported. Customers can access the \[control plane] through the AWS Management Console, AWS CLI, or AWS SDKs. To get started, visit the EventBridge \[Control Plane] documentation at aws.amazon.com/eventbridge/control-plane.

***

## FAQ

### Customer FAQs

**Q1: What is the EventBridge \[Control Plane] and how does it help me build event-driven applications?**

The EventBridge \[Control Plane] is a new AWS service that eliminates the complexity of building Event-Driven Architectures (EDA) using Amazon EventBridge, particularly when your applications need to consume events from different AWS accounts. Instead of spending days configuring cross-account event delivery infrastructure, you can simply subscribe to events and start processing them within minutes.

The \[control plane] provides domain abstraction that hides complex routing infrastructure from developers. When you want to consume events from another team's services, you use simple "subscribe" and "unsubscribe" API operations rather than manually creating EventBridge rules, managing IAM policies, and configuring cross-account delivery. All the underlying infrastructure is automatically provisioned and managed for you.

**Q2: How much does the EventBridge \[Control Plane] cost?**

The EventBridge \[Control Plane] follows AWS's pay-as-you-use pricing model with no upfront costs. You are charged based on four pricing dimensions:

* Domain registration: \$XXX per registered domain per month
* Event subscriptions: \$XXX per active subscription per month
* Event processing: Standard EventBridge pricing applies for event ingestion and delivery (\$1.00 per million events)
* Schema registry usage: \$XXX per schema version stored per month

There are no additional charges for the automated resource provisioning, cross-account delivery management, or monitoring capabilities. The \[control plane] is designed to reduce your overall EDA infrastructure costs by eliminating the overhead of managing multiple EventBridge resources manually.

**Q3: What regions is the EventBridge \[Control Plane] available in?**

The EventBridge \[Control Plane] launches simultaneously in all AWS regions where Amazon EventBridge is currently supported, including all commercial regions and GovCloud. Cross-account event delivery works seamlessly across regions, so you can have domains registered in one region consuming events from producers in different regions.

For the most up-to-date list of supported regions, visit the EventBridge \[Control Plane] documentation at aws.amazon.com/eventbridge/control-plane.

**Q4: How do I get started with the EventBridge \[Control Plane]?**

Getting started involves three simple steps:

First, register your domain through the EventBridge \[Control Plane] console, AWS CLI, or SDKs. This creates all necessary infrastructure including domain-specific event buses, IAM permissions, and monitoring.

Second, browse available events in the schema registry to discover what events other domains are publishing that you have permission to consume.

Third, use the subscribe API to connect your application to the events you need. The \[control plane] handles all cross-account delivery automatically.

Complete step-by-step tutorials and code examples are available in our getting started guide at aws.amazon.com/eventbridge/control-plane/getting-started.

**Q5: Can I use the \[Control Plane] with my existing EventBridge resources?**

Yes, the EventBridge \[Control Plane] is designed to work alongside your existing EventBridge implementations. You can migrate existing event buses and rules into the \[control plane] domain structure using our migration tooling, or run both approaches in parallel during your transition period.

For existing custom EventBridge configurations, the \[control plane] provides import capabilities that can analyze your current setup and recommend the optimal domain structure. This ensures you maintain all existing functionality while gaining the benefits of simplified cross-account management.

**Q6: What are the limitations I should be aware of?**

The EventBridge \[Control Plane] has several design limitations to be aware of:

Currently, the \[control plane] focuses on cross-account event scenarios. If all your event producers and consumers are within the same AWS account, you may not need the additional abstraction layer.

Domain registration requires appropriate IAM permissions across the AWS accounts where domain resources will be provisioned. Your organization's AWS account structure and permission policies may need updates to support \[control plane] operations.

Event replay capabilities are limited to 30 days of event history per domain. For longer-term event replay requirements, you'll need to implement additional storage and replay mechanisms.

The \[control plane] integrates with EventBridge's existing rule limit of 2000 per event bus, though our domain distribution approach significantly reduces the likelihood of reaching this limit.

### Internal FAQs

**Q7: What customer problem does the EventBridge \[Control Plane] solve?**

The EventBridge Control Plane solves the overwhelming complexity that AWS developers and architects face when building Event-Driven Architectures using Amazon EventBridge, particularly for cross-account event consumption. Today's customers encounter several specific technical barriers that prevent them from adopting EDA at scale.

First, cross-account event delivery requires extensive manual configuration. When event consumers are in different AWS accounts from event producers, developers must create two rules per event - one to send the event from the central event bus to the consumer's event bus, and another to process the event within the consumer account. This also requires configuring two separate IAM policies - one on the central event bus allowing consumers to create rules, and another on the consumer event bus permitting EventBridge to deliver events. For applications consuming hundreds of events, this becomes unmanageable.

Second, EventBridge's 2000 rule limit per event bus forces platform teams to create multiple event buses in central accounts, but this exposes routing infrastructure to consumers. Developers must now understand not only what events they want to subscribe to, but also which specific event bus contains those events. This coupling between consumers and routing infrastructure breaks fundamental EDA patterns.

Third, developers must manage additional integration resources including schemas and schema registries, event versioning, SQS queues for failure handling, and monitoring across multiple accounts and services. Flight Centre's Cloud Architect David captured this frustration: "We don't have a team big enough, or the skills to manage all these resources, we just want AWS to do that."

These complexity barriers reduce integration resource efficiency by requiring developers to manage cross-account delivery infrastructure instead of focusing on application logic that processes events.

**Q2: How does the EventBridge Control Plane solve these problems?**

The EventBridge Control Plane eliminates cross-account complexity through three key architectural innovations that abstract infrastructure management away from developers.

First, we provide domain abstraction that logically groups events without exposing underlying routing infrastructure. Platform teams register domains through the control plane, which automatically provisions domain-specific event buses in both producer and platform accounts, creates all necessary IAM permissions for cross-account event delivery, establishes domain-specific schema registries, and implements dead letter queues and CloudWatch monitoring for each domain. This eliminates the need for developers to understand or manage the underlying event routing infrastructure.

Second, we offer simple subscribe and unsubscribe API operations that handle all cross-account delivery complexity automatically. When developers subscribe to an event through the control plane, we create or remove rules on the appropriate domain-specific event buses and ensure consistency of inter-bus, inter-domain communication. Developers only interact with their own domain event bus, not the entire event flow infrastructure.

Third, we solve the EventBridge rule limit constraint by automatically distributing events across domain-specific buses while maintaining a unified developer experience. The \[control plane] manages rule distribution and event routing across multiple buses transparently, so developers never need to know which specific event bus contains the events they want to consume.

This separation of concerns allows platform teams to manage the vast majority of EDA integration infrastructure while giving development teams the freedom to create applications without relying on platform team support for each event subscription.

**Q3: How will you measure success?**

We will measure success through four key metrics that directly reflect customer value creation, in priority order:

1. **Developer productivity improvement**: We expect to reduce integration resource management overhead by XX% as measured by the number of AWS resources developers must configure per event subscription. Today developers configure an average of eight resources per event; our target is reducing this to three resources through automated provisioning.

2. **Time to event consumption**: We will track the time from a developer deciding to consume an event to successfully processing their first event in their application. Our goal is to reduce this from the current 1-2 days to under 2 minutes for 90% of use cases.

3. **EDA adoption rate**: We will measure the number of new event-driven applications built by AWS customers and the growth rate of cross-account event subscriptions. We expect to see XX% increase in new EDA implementations within six months of launch.

4. **Customer satisfaction**: Through regular customer surveys and Net Promoter Score tracking specifically for EventBridge EDA implementations, we target achieving an NPS of XXX within the first year, compared to current EDA customer satisfaction scores of XX.

**Q4: What is your North Star vision for the EventBridge Control Plane?**

Our North Star vision is to make Event-Driven Architecture as simple to implement as REST API integration, enabling any AWS developer to build resilient, scalable event-driven applications without infrastructure expertise.

Within three years, we envision the EventBridge Control Plane becoming the standard way enterprises implement EDA on AWS, supporting XX+ domain registrations and processing five billion events monthly across cross-account scenarios. We project this will enable XX% of AWS customers building new applications to adopt event-driven patterns, compared to the current XX% adoption rate.

The long-term vision extends beyond simplifying existing EventBridge capabilities to providing intelligent event discovery, automated schema evolution management, event replay capabilities across domains, and integration with AWS's broader serverless application development ecosystem. We aim to position AWS as the clear leader in cloud-native event-driven architecture implementation.

**Q5: What data informs this vision?**

Our vision is informed by extensive customer research, usage pattern analysis, and direct feedback from enterprise customers building event-driven applications on AWS.

Customers such as **PostNL(XL)** and **Woodside(XL)**, have already built their own customer solutions to adopt EDA and simplify integrations, and offer the service as a platform feature. 

Customer interviews with XX+ AWS customers revealed that XX% of organizations attempting to implement EDA with EventBridge abandon or significantly scale back their projects due to cross-account complexity. Internal AWS usage data shows that customers building event-driven applications spend XX hours on average configuring cross-account event delivery for each new event type they consume.

Flight Centre's experience exemplifies this challenge. Their development teams previously spent 1-2 days per event subscription managing IAM policies, cross-account delivery rules, and integration resources. In our early beta testing, the same teams completed event subscriptions in under 2 minutes using the control plane APIs.

AWS Solutions Architects report that cross-account event delivery complexity is the primary reason cited by customers for choosing alternative event streaming platforms over EventBridge for enterprise EDA implementations. Post-implementation surveys show that XX% of customers who successfully deploy EDA using current EventBridge capabilities wish they had a simpler way to manage cross-account scenarios.

**Q6: What are the biggest risks, and how will you mitigate them?**

The primary risks center on enterprise adoption challenges and technical scalability constraints as usage grows.

Our biggest risk is that large enterprise customers with existing complex EDA implementations may perceive the control plane as too abstract or constraining compared to their current custom solutions. To mitigate this, we are implementing comprehensive migration tooling that can import existing EventBridge configurations into control plane domain structures, and providing extensive customization options for enterprise-specific requirements.

Technical scalability presents another significant risk as the control plane must manage potentially millions of cross-account event subscriptions and automatically provision resources across thousands of AWS accounts. We are addressing this through distributed control plane architecture that can horizontally scale resource provisioning operations and implementing intelligent caching and batching for resource creation workflows.

Security and compliance risks emerge from automated cross-account resource provisioning, particularly for customers in regulated industries. We are mitigating this by implementing comprehensive audit logging for all control plane operations, providing fine-grained permission controls for domain registration and event subscription, and ensuring compliance with existing AWS security frameworks including CloudTrail integration and IAM policy validation.

**Q7: How could you go bigger?**

Beyond simplifying EventBridge cross-account scenarios, the control plane architecture creates opportunities for transforming how customers implement distributed application architectures on AWS.

We could extend domain abstraction to other AWS services, enabling unified management of event streams (Kinesis), message queues (SQS), and pub/sub messaging (SNS) through the same domain-based interface. This would position the control plane as AWS's unified asynchronous messaging platform rather than just an EventBridge enhancement.

Integration with AWS's application development ecosystem presents significant expansion potential. We could provide native integration with AWS SAM and CDK for infrastructure-as-code event subscription management, automatic event-driven API Gateway trigger creation, and seamless Lambda function deployment triggered by event subscriptions.

Enterprise workflow orchestration represents another major opportunity. The control plane could evolve into a visual workflow designer where business stakeholders define event-driven business processes, and the platform automatically provisions all necessary AWS resources and cross-account integrations to implement those workflows.

**Q8: Who will you collaborate with to achieve this?**

Success requires close collaboration across multiple AWS service teams and external partner organizations.

Internally, we have established partnerships with the Amazon EventBridge service team for core integration capabilities, AWS IAM for cross-account permission automation, and AWS CloudFormation for resource provisioning workflows. The AWS Lambda team is collaborating on optimized event processing patterns, while the Amazon API Gateway team is working on unified event-driven API creation workflows.

Our enterprise customer advisory board includes representatives from **Flight Centre(XL)**, **Lego(XL)**, and **PostNL(XL)** who provide ongoing feedback on enterprise requirements and beta testing for new capabilities. These customers are also serving as reference accounts for other enterprise prospects evaluating the control plane.

AWS Solutions Architecture and Professional Services teams are critical partners for customer onboarding and migration support. We have trained XX+ Solutions Architects on control plane implementation patterns and established dedicated support channels for customers migrating from existing EDA implementations.

External systems integrator partnerships with Accenture, Deloitte, and Capgemini enable us to reach enterprise customers who prefer partner-led implementations while ensuring consistent deployment patterns across different customer environments.

**Q9: What other options did you consider?**

We evaluated several alternative approaches before settling on the domain abstraction model with automated resource provisioning.

Our first consideration was enhancing existing EventBridge capabilities with better cross-account tooling rather than building a separate control plane. However, this approach would still require developers to understand EventBridge's underlying complexity and wouldn't address the fundamental rule limit constraints that force customers to manage multiple event buses.

We also considered a configuration-driven approach where customers would define their EDA requirements in YAML or JSON templates, and AWS would generate the necessary EventBridge resources. While this would simplify initial setup, it wouldn't provide the ongoing subscription management and runtime abstraction that customers need for evolving event-driven applications.

A third option involved extending AWS Organizations to provide automatic EventBridge resource sharing across accounts within an organization. This would reduce some cross-account configuration complexity but wouldn't address the fundamental developer experience problems of managing rules, schemas, and event routing logic.

We ultimately chose the domain abstraction approach because it provides the greatest separation between infrastructure management and application development concerns, enabling both platform teams and development teams to operate independently while maintaining enterprise-grade security and compliance capabilities.

**Q10: How does this accelerate AWS's event-driven architecture strategy?**

The EventBridge \[Control Plane] directly accelerates AWS's strategic goal of becoming the leading cloud platform for event-driven application development by removing the primary adoption barrier preventing customers from implementing EDA at enterprise scale.

Currently, AWS customers building event-driven applications often choose competitor platforms like Confluent or Apache Kafka because EventBridge's cross-account complexity makes it impractical for large-scale implementations. The control plane eliminates this competitive disadvantage by providing developer experience parity with these alternatives while maintaining EventBridge's native AWS service integrations and serverless operational model.

The control plane also strengthens AWS's broader serverless strategy by enabling customers to build sophisticated event-driven serverless applications without infrastructure management expertise. This expands the addressable market for AWS Lambda, API Gateway, and other serverless services beyond customers with deep AWS infrastructure knowledge.

For AWS's enterprise customer segments, the control plane provides a compelling migration path from on-premises message brokers and ESB implementations to cloud-native event-driven architectures. This supports AWS's strategic objective of accelerating enterprise cloud adoption by offering modern alternatives to legacy integration platforms.

Finally, the domain abstraction model creates opportunities for AWS to provide higher-level business process automation capabilities built on event-driven foundations, positioning AWS for future growth in workflow automation and business process management markets.

***

## Appendices

### A. Key Performance Indicators

* Developer resource configuration reduction: XX% fewer AWS resources managed per event subscription
* Time to event consumption: <2 minutes for 90% of use cases (vs. current 1-2 days)
* EDA adoption rate: 1XX% increase in new implementations within 6 months
* Customer satisfaction: NPS of XX within first year

### B. Customer Testimonials

**Flight Centre - David, Cloud Architect**: "We don't have a team big enough, or the skills to manage all these resources, we just want AWS to do that."

**Flight Centre Developer**: "Before, I had to spend 1-2 days managing access to event buses, IAM policies and cross account delivery, and define rules for subscriptions and application integration, just to consume an event. Now, all I need to do is subscribe to an event and the cross account delivery is invisible to me. In minutes I can start processing events for my application."

### C. Technical Architecture Summary

The EventBridge \[Control Plane] implements domain abstraction through:

* **Automated provisioning of domain-specific event buses** - creating dedicated event buses for each domain in both producer and consumer accounts, eliminating the need for manual resource creation and management
* **Cross-account IAM policy management** - automatically configuring and maintaining the complex permission structures required for secure event delivery between AWS accounts
* **Schema registry creation and management** - establishing domain-specific schema registries with proper access controls and versioning capabilities for event structure governance
* **Version management abstraction** - automatically handling schema versioning, impact analysis, and providing migration plans for consumers to adopt new schema versions
* **Event flow mapping abstraction** - providing comprehensive visibility into event origins, consumption patterns, and cross-domain relationships throughout the organization
* **Dead letter queue implementation** - automatically configuring failure handling mechanisms for each domain to ensure reliable event processing and troubleshooting capabilities
* **CloudWatch monitoring setup** - establishing comprehensive observability with metrics, alarms, and dashboards for domain-level event processing and system health
* **Simple publish, subscribe, and unsubscribe API operations** - providing intuitive APIs that abstract away complex rule creation and event routing management, enabling event producers to publish without knowing target buses and consumers to focus on business logic

### D. Competitive Analysis

Current alternatives require customers to manage:

* Multiple EventBridge rules per event subscription
* Cross-account IAM policies
* Event bus routing logic
* Schema registry maintenance
* Failure handling configuration

The control plane abstracts all these concerns while maintaining EventBridge's native AWS integrations and serverless operational model.
