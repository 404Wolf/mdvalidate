#import "@preview/glossarium:0.5.9": make-glossary, register-glossary, print-glossary, gls, glspl
#show: make-glossary

#let entry-list = (
  (
    key: "kuleuven",
    short: "KU Leuven",
    long: "Katholieke Universiteit Leuven",
    description: "A university in Belgium.",
  ),
)
#register-glossary(entry-list)

#align(center + horizon)[
  #v(-10em)

  #text(size: 24pt, weight: "bold")[
    Software Requirements Specification for _WebRTCDroid_
  ]

  #v(1em)

  #text(size: 14pt)[
    Prepared by Wolf Mermelstein and Alesandro Mason
  ]

  #v(1em)

  #text(size: 16pt, weight: "semibold")[
    Case Western Reserve University
  ]

  #v(1em)

  #text(size: 14pt)[
    September 14, 2025
  ]
]

#pagebreak()

#outline(depth: 3)

#pagebreak()

= Introduction

== Purpose // alesandro

// Describe the purpose of this SRS and its intended audience.

This SRS lays out the scope of our WebRTC android-in-the-browser project, its
features, and the general architecture. The intended audience is both developers
looking to contribute, and potential API consumers of the project.

== Document Conventions // wolf

// Describe any standards or typographical conventions that were followed when
// writing this SRS, such as fonts or highlighting that have special
// significance. For example, state whether priorities for higher-level
// requirements are assumed to be inherited by detailed requirements, or whether
// every requirement statement is to have its own priority.

We are laying out a general outline of a effort to get physical android devices
streamed over WebRTC. Some of the specifications we provide are general bulleted
lists of features or specification that aren't totally complete, and are
simplifications to stay focused to the primary goal of the project.

== Intended Audience and Reading Suggestions // wolf

// Describe the different types of reader that the document is intended for,
// such as developers, project managers, marketing staff, users, testers, and
// documentation writers. Describe what the rest of this SRS contains and how it
// is organized. Suggest a sequence for reading the document, beginning with the
// overview sections and proceeding through the sections that are most pertinent
// to each reader type.

This document serves as a technical outline for technical of the project for
Android DevOps engineers.

== Project Scope

// Provide a short description of the software being specified and its purpose,
// including relevant benefits, objectives, and goals. Relate the software to
// corporate goals or business strategies. If a separate vision and scope
// document is available, refer to it rather than duplicating its contents here.
// An SRS that specifies the next release of an evolving product should contain
// its own scope statement as a subset of the long-term strategic product
// vision.

*Primary Objectives:*
- Remote device control for development teams without hardware dependencies  
- Cost-effective alternative to physical device testing infrastructure
- Real-time collaborative testing and demonstration capabilities
- Automated CI/CD pipeline integration for mobile app testing at scale

*Business Value:*
  Improving development velocity through instant device provisioning and
  parallel testing capabilities. Supports corporate digital transformation
  initiatives by enabling distributed mobile development teams.
  *Competitors*: The current market leader with their starter plan at 708 per year and their premium plan at 3828 per year

Addresses the growing demand for mobile-first applications by providing
  scalable testing infrastructure that supports continuous delivery practices
  and remote development workflows.

== References // wolf

// List any other documents or Web addresses to which this SRS refers. These may
// include user interface style guides, contracts, standards, system
// requirements specifications, use case documents, or a vision and scope
// document. Provide enough information so that the reader could access a copy
// of each reference, including title, author, version number, date, and source
// or location.

= Overall Description

== Product Perspective // wolf

The tool being developed functions as a collection of standalone components that
the actual "product" we will end up building composes into a undified demo.

The android developer tooling ecosystem currently is relatively scattered.

#figure(
  image("images/avd-manager.png", width: 50%),
  caption: [Android Studio Virtual Device Manager GUI]
)

To configure a virtual android device for developing Android applications you
often will use [Android Studio](https://developer.android.com/studio), which is
a Jetbrains based GUI that provides a fully integrated developer environment for
making Android applications. It has a UI where you can configure an Android
virtual device, download Android images that you can run as emulators, and
generally provides a wrappers on Google's official Android CLIs (like
`avdmanager` and `emulator`). Android Studio also has the ability to actually
program Android applications and provides a side by side emulator, much like
Apple's XCode integration for writing Swift IOS applications.

This type of tooling is, however, designed for end users that are programming
applications, and our focus is more on automated pipelines, application
consumers, and providing a more editor agnostic/generic tool for using
virtualized Android devices. Google maintains a similar tool to offer a "fully
integrated" experience for browsers, as part of [Project IDX](https://idx.dev/)
(now known as Fireship Studio). This tool motivated Google to add native WebRTC
support for android.

One of the tools that we will be using to implement our project, which achieves
a similar objective but is designed mostly for local use and not for browser
compatibility, is [Screen Copy](https://github.com/Genymobile/scrcpy). Screen
copy lets you stream your Android device to their custom client that runs on
your desktop, and can handle streaming audio, video, and control inputs.

// Describe the context and origin of the product being specified in this SRS.
// For example, state whether this product is a follow-on member of a product
// family, a replacement for certain existing systems, or a new, self-contained
// product. If the SRS defines a component of a larger system, relate the
// requirements of the larger system to the functionality of this software and
// identify interfaces between the two. A simple diagram that shows the major
// components of the overall system, subsystem interconnections, and external
// interfaces can be helpful.

== Product Features // alesandro

// Summarize the major features the product contains or the significant
// functions that it performs or lets the user perform. Details will be provided
// in Section 3, so only a high level summary is needed here. Organize the
// functions to make them understandable to any reader of the SRS. A picture of
// the major groups of related requirements and how they relate, such as a top
// level data flow diagram or a class diagram, is often effective.

== User Classes and Characteristics // alesandro

// Identify the various user classes that you anticipate will use this product.
// User classes may be differentiated based on frequency of use, subset of
// product functions used, technical expertise, security or privilege levels,
// educational level, or experience. Describe the pertinent characteristics of
// each user class. Certain requirements may pertain only to certain user
// classes. Distinguish the favored user classes from those who are less
// important to satisfy.
*Primary Users - DevOps Engineers (High Priority):*
- Frequency: Daily automated usage via CI/CD pipelines
- Technical expertise: Advanced scripting and infrastructure management
- Key requirements: API reliability, horizontal scaling, integration capabilities
- Usage patterns: Batch testing, parallel device provisioning, automated deployment validation

*Secondary Users - Mobile Developers (High Priority):*
- Frequency: Regular interactive usage during development cycles
- Technical expertise: Mobile development proficiency, moderate infrastructure knowledge
- Key requirements: Low-latency interaction, device variety, debugging capabilities
- Usage patterns: Interactive testing, APK deployment, collaborative debugging sessions

*Tertiary Users - QA Teams (Medium Priority):*
- Frequency: Periodic manual testing campaigns
- Technical expertise: Testing methodology focus, basic technical skills
- Key requirements: User-friendly web interface, session sharing, test result capture
- Usage patterns: Manual testing workflows, bug reproduction, stakeholder demonstrations

*Administrative Users - Platform Operators (Medium Priority):*
- Frequency: Ongoing system monitoring and maintenance
- Technical expertise: System administration and security management
- Key requirements: Monitoring dashboards, user management, resource optimization
- Usage patterns: System health monitoring, user provisioning, capacity planning

== Operating Environment // wolf

// Describe the environment in which the software will operate, including the hardware platform,
// operating system and versions, and any other software components or applications with which it
// must peacefully coexist.

== Design and Implementation Constraints // wolf

// Describe any items or issues that will limit the options available to the developers. These might
// include: corporate or regulatory policies; hardware limitations (timing requirements, memory
// requirements); interfaces to other applications; specific technologies, tools, and databases to be
// used; parallel operations; language requirements; communications protocols; security
// considerations; design conventions or programming standards (for example, if the customer's
// organization will be responsible for maintaining the delivered software).

== User Documentation // wolf

// List the user documentation components (such as user manuals, on-line help, and tutorials) that
// will be delivered along with the software. Identify any known user documentation delivery formats
// or standards.

== Assumptions and Dependencies // wolf

// List any assumed factors (as opposed to known facts) that could affect the requirements stated in
// the SRS. These could include third-party or commercial components that you plan to use, issues
// around the development or operating environment, or constraints. The project could be affected if
// these assumptions are incorrect, are not shared, or change. Also identify any dependencies the
// project has on external factors, such as software components that you intend to reuse from
// another project, unless they are already documented elsewhere (for example, in the vision and
// scope document or the project plan).

= System Features // 

// This template illustrates organizing the functional requirements for the product by system
// features, the major services provided by the product. You may prefer to organize this section by
// use case, mode of operation, user class, object class, functional hierarchy, or combinations of
// these, whatever makes the most logical sense for your product.
//
// - Getting a screen and audio streamed to a react component in a web browser.
// - Creating a programmatic way to "spin up" an emulator with an HTTP API.
// - Making a front end web appication to use that endpoint and show your running device.
// - Allow having multiple emulators that are running at the same time.
// - Sandboxing a android phone in a docker container *with a mounted SD card*.
// - Supporting controlling the device from the browser over a WebRTC data stream.
// - "Multiplayer" (many people viewing the same device with a share link).
// - "Multiplayer" and allowing many people to **control** the same device.
// - Deploying to edge containers like fly.io OR building a multitenent system on a bare metal instance.

== Priority Implementation Order

+ WebRTC video/audio streaming to browser
+ HTTP API for emulator lifecycle management
+ Web dashboard for emulator access and control
+ Docker containerization with SD card mounting
+ Browser-based device control via WebRTC data channels

_Time permitting_

+ Production deployment (edge or bare metal)
+ Multi-instance concurrent emulator support
+ Multiplayer viewing with shareable links
+ Collaborative control with multiple users

== Technical Stack

- *Backend*:
  - Podman OCI runtime with Nix image building
  - Go with Pion WebRTC library
  - REST API, future gRPC migration

- *Frontend*:
  - Vanilla react built with Vite
  - WebRTC with adapter.js or native WebRTC browser API

- *Android*:
  - Official emulators
  - Custom image support (LineageOS via Robotnix)

== REST API to manage android instances

We will have a REST API to manage android instances. The API will allow users
to create, start, stop, and delete android instances. The API will also allow
users to get the status of an instance.

=== Description and Priority

We will have a REST API that can manage android images and instances. It will
offer the ability to:

High Priority:

+ Create new Android instance that is instantly running, and return an ID
  associated with that and get a URL to connect to it.

Medium Priority:

+ Create an image and virtual device, with configuration options (device type,
  Android version, etc).

Low Priority:

+ Virtual SD card support (so you can "transfer" state between instances).
+ Stop an instance given an instance ID.
+ Get the status of an instance given an instance ID.
+ Get logs associated with an emulator instance.
+ Pause and resume instances (preserving the state of the instance).

=== Stimulus/Response Sequences

Generally, the flow to create and connect to a virtual Android device will be:

1. User clicks a "Make image" button on the web app. This sends a request that
   looks like:

```
HTTP POST /images
Content-Type: application/json
{
  "label": "my-android-image",
  "type": "android",
  "android_version": "11",
}
```

2. User clicks a "Create device" button on the web app. This sends a
   request that looks like:

```
HTTP POST /devices
Content-Type: application/json
{
  "resolution": {
    "width": 1080,
    "height": 2340,
    "density": 440
  },
  "label": "my-android-device"
}
```

For both 1. and 2. the server responds with a JSON that has a unique ID for the
image and device.

3. The user then clicks a "Start instance" button on the web app. This sends a
   request that looks like:

```
HTTP POST /instances
Content-Type: application/json
{
  "image_id": "image-uuid",
  "device_id": "device-uuid"
}
```

=== Functional Requirements

// Itemize the detailed functional requirements associated with this feature. These are the
// software capabilities that must be present in order for the user to carry out the
// services provided by the feature, or to execute the use case. Include how the product
// should respond to anticipated error conditions or invalid inputs. Requirements should
// be concise, complete, unambiguous, verifiable, and necessary. Use "TBD" as a
// placeholder to indicate when necessary information is not yet available.

// Each requirement should be uniquely identified with a sequence number or a meaningful
// tag of some kind.

// REQ-1:
// REQ-2:

== Live video, audio, and interaaction streaming

// Additional system features follow the same structure as System Feature 1

== Web App to interface with the device manager

// Additional system features follow the same structure as System Feature 1

= External Interface Requirements

== Software Interfaces // wolf

We will be making a `go` server that speaks `Scrcpy`'s binary protocol and
re-exposes the phone over a WebRTC API so that you can communicate with the
phone via a web browser.

The `go` server will establish TCP connections to the `scrcpy` server running on
the Android device, and then, using [pion](https://github.com/pion/webrtc), a
`go` implementation of WebRTC, re-expose the phone over WebRTC.

// Describe the connections between this product and other specific software components (name
// and version), including databases, operating systems, tools, libraries, and integrated commercial
// components. Identify the data items or messages coming into the system and going out and
// describe the purpose of each. Describe the services needed and the nature of communications.
// Refer to documents that describe detailed application programming interface protocols. Identify
// data that will be shared across software components. If the data sharing mechanism must be
// implemented in a specific way (for example, use of a global data area in a multitasking operating
// system), specify this as an implementation constraint.

= Other Nonfunctional Requirements // 

== Performance Requirements // 

// If there are performance requirements for the product under various circumstances, state them
// here and explain their rationale, to help the developers understand the intent and make suitable
// design choices. Specify the timing relationships for real time systems. Make such requirements as
// specific as possible. You may need to state performance requirements for individual functional
// requirements or features.

- Concurrent emulators: n+ per server node
- Streaming latency: $< n m s$ end-to-end
- Container startup: $\<n $ seconds
- Multi-viewer support: 50 viewers, 10 controllers per instance

== Safety Requirements // 

// Specify those requirements that are concerned with possible loss, damage, or harm that could
// result from the use of the product. Define any safeguards or actions that must be taken, as well as
// actions that must be prevented. Refer to any external policies or regulations that state safety
// issues that affect the product's design or use. Define any safety certifications that must be
// satisfied.
// 
- Container isolation with read-only filesystems
- Encrypted WebRTC communications (DTLS/SRTP)
- Resource quotas and automatic cleanup
- Session-based access controls

== Security Requirements // alessandro

// Specify any requirements regarding security or privacy issues surrounding use of the product or
// protection of the data used or created by the product. Define any user identity authentication
// requirements. Refer to any external policies or regulations containing security issues that affect
// the product. Define any security or privacy certifications that must be satisfied.


== Software Quality Attributes // 

// Specify any additional quality characteristics for the product that will be important to either the
// customers or the developers. Some to consider are: adaptability, availability, correctness, flexibility,
// interoperability, maintainability, portability, reliability, reusability, robustness, testability, and
// usability. Write these to be specific, quantitative, and verifiable when possible. At the least, clarify
// the relative preferences for various attributes, such as ease of use over ease of learning.
// 
- *Reliability*: 99.9% uptime with automatic recovery
- *Scalability*: Linear performance scaling to design limits  
- *Usability*: 5-minute learning curve for new users
- *Maintainability*: >80% test coverage, daily deployment capability

= Other Requirements

// Define any other requirements not covered elsewhere in the SRS. This might include database
// requirements, internationalization requirements, legal requirements, reuse objectives for the
// project, and so on. Add any new sections that are pertinent to the project.

= Appendix A: Glossary

// Define all the terms necessary to properly interpret the SRS, including acronyms and
// abbreviations. You may wish to build a separate glossary that spans multiple projects or the entire
// organization, and just include terms specific to a single project in each SRS.

#print-glossary(
  entry-list,
  show-all: true,
)

= Appendix B: Analysis Models 

// Optionally, include any pertinent analysis models, such as data flow diagrams, class diagrams,
// state-transition diagrams, or entity-relationship diagrams.

#figure(image("images/overall-system.svg"))

= Appendix C: Issues List // wolf

// This is a dynamic list of the open requirements issues that remain to be resolved, including
// TBDs, pending decisions, information that is needed, conflicts awaiting resolution, and the like.
