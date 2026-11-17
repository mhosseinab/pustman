# Engineering handoff template

## Release/slice

Name: [fill in]  
Figma file and selected frames: [actual links]  
Prototype: [actual link, simulated/implemented scope]  
Target platform/build: [fill in]  
Approved scope and revision: [fill in]

## Behavior contract

For each requirement, reference the screen ID, state matrix row, fixture, component and acceptance criterion. Describe what happens after success, failure, cancellation, offline changes and lost permissions. Identify any behavior requiring a backend guarantee that the prototype cannot validate.

## Components and tokens

Record the actual main-component IDs, text styles, variables and modes. Map to code components after implementation begins. Do not claim Code Connect mappings exist unless they were created and checked.

## Collaboration contract

Record effective permission sources; local/shared data boundaries; comment anchors; review-versus-merge rules; conflict resolution strategy; notification recipients; audit/activity events; and environment/secret ownership. Specify whether the slice uses Postman cloud forks or Git-native review.

## State/fixture map

| Requirement | Figma state/frame | Fixture | Implementation surface | Acceptance evidence | Status |
| --- | --- | --- | --- | --- | --- |
| [ID] | [actual node link] | [scenario ID] | [component or service] | [test/observation] | [not started / ready / blocked] |

## Open questions and deliberate deviations

For each unresolved point: decision owner, options, impact, due gate and source. List deliberate differences from current Postman separately from unknown behavior. Preserve deferred features in the capability ledger.

## Verification

List checks actually completed and the exact revision they covered. Record residual failures, plan/build limitations and user-study evidence. Include keyboard and assistive-technology results only when actually tested.
