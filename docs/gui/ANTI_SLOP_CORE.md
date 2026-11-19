# Anti-Slop Core — MCW GUI Quality Filter

## Purpose

This document filters out generic, careless, or superficially polished UI work. It is not a style guide and does not prescribe a specific palette, font, framework, or layout. `DESIGN.md` provides MCW's visual direction.

## Tier 1 — Hard gates

A failure here blocks delivery.

1. **Truthful UI:** no fabricated metrics, fake progress, misleading statuses, or success states shown before work is complete.
2. **Functional affordances:** every visible control must work or be clearly disabled with an explanation when needed.
3. **State completeness:** important workflows must handle loading, empty, success, error, and cancellation states as applicable.
4. **Data safety:** destructive or overwriting actions must communicate impact and use appropriate confirmation/recovery.
5. **No broken layout:** realistic names, paths, versions, long text, and supported window sizes must not break core tasks.
6. **Input access:** primary workflows must be keyboard reachable, with visible focus where supported.
7. **Honest reporting:** never claim visual, accessibility, or test verification that was not performed.
8. **Architecture integrity:** UI must respect the Rust Edition's actual state ownership and domain boundaries.

## Tier 2 — Purpose gates

For every prominent visual or interaction choice, ask:

- What user task does this help?
- What information does it clarify?
- Why is this treatment better than a simpler option?
- Is it consistent with `DESIGN.md`?
- Does it survive realistic content and window sizes?

A card, gradient, shadow, animation, icon, badge, texture, or illustration is allowed when it has a clear job. Remove it when it exists only to make the screen look more "designed".

## Tier 3 — Quality locks

1. **Hierarchy:** users can identify context, primary information, and primary action quickly.
2. **Consistency:** shared components look and behave alike.
3. **Specificity:** the UI reflects launcher workflows rather than a generic admin template.
4. **Restraint:** visual emphasis is reserved for meaningful differences.
5. **Liveliness:** the product should not become sterile. Personality may come from typography, illustration, micro-interactions, or a distinctive layout when these are coherent and useful.
6. **Density:** information density matches the task; neither sparse for show nor cramped for efficiency.
7. **Craft:** alignment, spacing, truncation, focus, transitions, and error copy are deliberate.
8. **Performance:** motion, rendering, and large assets must not harm responsiveness.

## Liveliness check

Avoid solving anti-slop by removing all personality. Ask:
- Is there one recognizable visual signature?
- Do meaningful states feel responsive?
- Does the interface have a clear rhythm?
- Is the result pleasant without competing with the task?
- Does personality come from product-specific choices rather than random decoration?

## Delivery gate

Before shipping, classify each section:
- **PASS:** verified and meets the criterion.
- **FAIL:** known defect; list severity and impact.
- **NOT VERIFIED:** not tested or not observable in this environment.

Do not convert NOT VERIFIED into PASS. See `DELIVERY_REPORT.md`.
