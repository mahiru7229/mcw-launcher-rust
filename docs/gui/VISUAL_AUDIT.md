# Visual Audit — Findings-First Mode

Use this when reviewing an existing GUI without immediately changing it.

## Audit procedure

1. Identify the screens and user journeys in scope.
2. Run the application or inspect available screenshots at realistic window sizes.
3. Check `DESIGN.md`, shared tokens, and existing component conventions.
4. Walk through primary and failure states.
5. Test keyboard/focus and resizing where possible.
6. Apply `ANTI_SLOP_CORE.md`.
7. Produce prioritized findings with evidence.
8. Wait for approval before broad redesign or architectural changes.

## Finding format

For each finding include:
- **ID:** GUI-001
- **Severity:** Critical / High / Medium / Low
- **Location:** screen/component
- **Evidence:** screenshot, reproduction, or code location
- **Problem:** observable issue, not personal taste
- **User impact:** task, error, confusion, or accessibility consequence
- **Recommendation:** smallest effective improvement
- **Confidence:** High / Medium / Low

## Severity guide

- **Critical:** blocks a primary workflow, risks user data, or makes a key action unusable.
- **High:** causes substantial confusion, repeated errors, or major accessibility failure.
- **Medium:** creates friction or inconsistency in a common workflow.
- **Low:** polish issue with limited impact.

## Audit boundaries

Do not label a design "bad" solely because it uses a gradient, card, shadow, animation, or a style you personally dislike. Evaluate purpose, consistency, usability, and alignment with `DESIGN.md`.

## Final audit summary

List findings by severity, evidence collected, areas not inspected, and recommended order of fixes. Clearly separate verified defects from subjective opportunities.
