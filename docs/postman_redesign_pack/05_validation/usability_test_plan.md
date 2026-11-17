# Usability and collaboration evaluation

This is a proposed study plan. No task times, interviews or user outcomes were collected for this pack.

## Setup

Start with 5-8 representative participants, including at least two pairs who already share API work. Include different experience levels and someone who regularly uses keyboard navigation or assistive technology when possible. This is formative research, not a statistical benchmark.

Use the same tasks and fixtures in the current product and the prototype. Counterbalance the order to reduce learning effects. Record the actual app version, platform, viewport, theme, account role and environment. Explain that the prototype uses synthetic data and has no real team or network connection.

Do not teach participants where controls are. Ask them to think aloud; give neutral prompts such as "What are you looking for?" and record where help was necessary.

## Tasks

| ID | Task prompt | Success observation | Failure to watch |
| --- | --- | --- | --- |
| U01 | Find List orders and tell me where Send will send it. | Correct request, environment and resolved URL identified | Environment guessed from color or workspace name |
| U02 | Send the request and find the amount of order ord_1042. | Finds amount_minor=12400 and currency=EUR | Confuses amount units or reads a stale response |
| U03 | The token has expired. Get the request working using the demo credential setup. | Finds inherited auth and changes the appropriate local credential | Changes shared auth unnecessarily or hides the response |
| U04 | This run returned 200 but one check failed. Explain the failure. | Separates HTTP status from assertion result | Treats green HTTP status as all tests passed |
| U05 | Change limit to 50 after sending. Which request produced the response below? | Recognizes execution snapshot is still limit 20 | Assumes the displayed response reflects unsent edits |
| U06 | As Sam, configure your token without changing it for Alex. | Edits a personal local value with shared values protected | Treats all read-only environment fields as inaccessible |
| U07 | As Jo, bring Sam into the correct workspace with view access. | Reviews identity, audience, role and pending state | Assumes pending invitation grants immediate access |
| U08 | Share the collection and environment; explain exactly what Sam receives. | Distinguishes link, permission, shared values and local exclusions | Assumes masked tokens are not shared |
| U09 | Alex requests Mina's review. Mina explains the change and asks for a correction. | Finds diff, revision, test evidence and thread context | Reviews stale evidence or wrong fork |
| U10 | Resolve the conflicting limit values without losing either person's work. | Identifies base/source/target where available and previews result | Confuses source/target or accepts unexplained overwrite |
| U11 | As a viewer, try to edit shared source and find the correct access path. | Understands restriction and request-access scope | Assumes workspace role controls every object identically |
| U12 | Continue after edit access disappears while a draft is dirty. | Preserves local work and understands write failure | Believes the draft was saved to the team |
| U13 | Open a comment notification as a person who lacks access. | Finds request-access route without exposed comment body | Believes a mention grants access |
| U14 | Complete U01 and close an environment dialog using only the keyboard. | Logical focus sequence and correct focus return | Keyboard trap, invisible focus, inaccessible icon action |

For U09-U10 use two participants or two deterministic personas and label which is being tested. A persona switch tests UI states; it does not validate real concurrent networking.

## Record

Task completion: independently completed / completed with help / failed. Record time to meaningful completion, errors, environment mistakes, wrong-audience sharing, help requests, recovery success and a 1-5 ease rating. Capture direct observations and short quotes with consent; do not turn interpretations into quotes.

Use columns: participant_id, task_id, product_version, role, order, completion, seconds, error, evidence, severity, proposed_fix, retest_result.

## Severity rubric

- Critical: irreversible data loss, unintended sharing, wrong-environment action caused by unclear context, or a blocked primary workflow with no recovery.
- High: frequent task failure or incorrect interpretation of result, access, revision or review status.
- Medium: recoverable confusion or significant friction.
- Low: small consistency or polish issue without task failure.

## Exit decision

Fix critical failures before handoff. Retest high-impact changes on the tasks that exposed them. Compare against baseline rather than presenting an unmeasured "faster" claim. The proposed 20% time-improvement target is optional and must be evaluated with measured data. Do not broaden testing after the concrete remaining risks are resolved.
