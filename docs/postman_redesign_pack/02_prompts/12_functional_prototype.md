# 12 · Build a functional prototype

**Inputs:** Attach approved Figma frames through the tool's context/attachment flow; attach scenarios, collaboration spec, design guidelines and state matrix.  
**Scope:** A Figma Make file or a Code Layer on `08 Prototype`. Keep editable Design frames intact.  
**Review gate:** Reproducible behavior with clearly identified simulations.

The built-in Design agent's current documentation lists native prototype wiring as a pending capability. Use Make/Code Layers for this task, or wire native Figma interactions manually if that is the chosen format. Check current capability availability before running.

```text
Build a functional prototype from the attached approved frames and fixtures. Follow the operating contract, the collaboration spec and the design guidelines. Preserve the approved visual system and information architecture.

Support these reproducible tasks:
1. Select and edit a request, choose the environment, send and inspect nested JSON.
2. Show deterministic success, HTTP 401/500, unresolved-variable, network-failure and cancellation scenarios, then let the user recover.
3. Open a test result and inspect a failed assertion.
4. Switch the demonstration persona among Alex, Mina, Sam and Jo; enforce the fixture capabilities in the prototype UI.
5. Let Alex request Mina's review, let Mina comment/review, and show the specified outcome.
6. Demonstrate the specified conflict and shared-environment scenarios.

Use synthetic local fixture data. Do not connect real APIs, ask for credentials, send invitations/messages, or modify an actual team. Label persona switching as a demo control. Label Send responses and multi-person collaboration as simulated. A UI-only permission demonstration is not a production authorization system.

Make meaningful controls work. For unfinished controls, use a clear explanatory state; don't present silent no-op buttons as completed. Provide a Reset demo action and deterministic scenario selection so tasks can be repeated. Preserve edits when the specs require it.

Support keyboard use, visible focus, the two required desktop sizes, and both themes. If persistence is used, state its scope and provide reset. Keep any Make guidelines in the project's guidelines file.

Completion criteria: run each listed task and report what passed, failed, or remains simulated. Provide a scenario/task map and the real prototype link if available. Do not publish to a public URL or send it to anyone as part of this task.
```
