# MCW Launcher Rust Edition — GUI Agent Instructions

Apply these instructions whenever a task changes GUI layout, visual styling, interaction, copy, icons, accessibility, or UI architecture.

## 1. Start with the product

- Inspect the actual Rust GUI framework, component tree, state model, theme system, navigation, and existing screens.
- Read `DESIGN.md` before making visual changes.
- Read only the relevant `docs/gui/` references and skill files for the current task.
- Identify the user goal, primary action, key information, edge cases, and success state.
- Preserve existing product identity unless a redesign is explicitly requested.

## 2. Choose a working mode

- **During:** guide the work from discovery through implementation and finish with `docs/gui/DELIVERY_REPORT.md`.
- **Audit:** inspect existing UI and return prioritized findings with evidence. Do not implement broad changes until approved.
- If the user already specified a mode, do not ask again.

## 3. The design filter

Treat `docs/gui/ANTI_SLOP_CORE.md` as a quality filter, not a visual style guide. It does not ban gradients, shadows, cards, animation, illustrations, or bold typography categorically. It requires a concrete purpose, consistency, and evidence that each choice improves the product.

Aesthetic direction comes from `DESIGN.md`. If that file is incomplete, do not invent a permanent brand system. Use restrained, reversible choices and label the result as a draft direction.

## 4. Required workflow

1. **Discover:** inspect relevant screens, components, tokens, framework constraints, and existing UX.
2. **Define:** state user goal, primary action, information hierarchy, and acceptance criteria.
3. **Design:** describe the proposed structure and any meaningful visual decisions.
4. **Implement:** reuse existing components and state ownership; keep changes focused.
5. **Verify:** test realistic data, empty/loading/error/success states, keyboard behavior, resizing, and theme variants where supported.
6. **Review:** run the anti-slop filter and inspect rendered output at realistic desktop sizes.
7. **Report:** complete the delivery report with evidence and unresolved issues.

## 5. Non-negotiable quality rules

- Every visible control must do what its label or affordance promises.
- No fake metrics, fake progress, placeholder charts, or decorative status indicators.
- No visual treatment without a purpose.
- No arbitrary one-off colors, spacing, radii, or typography.
- No generic dashboard layout unless the user task genuinely requires one.
- No essential action hidden only in a context menu.
- No raw technical error dumped into ordinary user-facing copy.
- No UI thread blocking for long-running work.
- No duplicate business logic or competing sources of truth in presentation code.
- No claims of accessibility or visual verification without testing.

## 6. Rust and architecture constraints

- Follow the actual repository architecture and UI framework patterns.
- Keep domain/business rules out of presentation code where the architecture separates them.
- Respect async cancellation, task ownership, and error propagation.
- Avoid introducing UI dependencies without a clear justification.
- Do not assume the Python/PySide6 edition's architecture applies to Rust Edition.

## 7. Definition of done

A UI task is complete only when:
- The user goal and primary action are clear.
- The result follows `DESIGN.md` or is explicitly marked as a draft.
- Interactive states are coherent and functional.
- Realistic content and edge cases have been checked.
- Keyboard/focus and scaling have been considered.
- No unjustified visual decoration remains.
- The delivery report accurately records PASS, FAIL, or NOT VERIFIED.
