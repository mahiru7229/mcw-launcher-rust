# Design System Guidelines

## 1. Tokens first

Centralize values in the framework's established theme/token mechanism. Avoid magic numbers and per-screen styling drift.

Recommended semantic groups:
- `color.background`, `color.surface`, `color.text`, `color.text_muted`, `color.border`
- `color.accent`, `color.success`, `color.warning`, `color.danger`, `color.focus`
- `space.*`, `radius.*`, `type.*`, `control.*`, `motion.*`

Use semantic names rather than names tied to a particular shade.

## 2. Color

- Define a restrained neutral foundation and intentional accent roles.
- Use semantic status colors consistently.
- Do not rely on color alone to communicate state.
- Verify contrast in actual rendered conditions.
- Define hover, pressed, focus, selected, disabled, and error appearances.
- Support both themes if the product supports both.

## 3. Typography

- Use a small, deliberate type scale.
- Establish hierarchy with size, weight, and spacing.
- Avoid excessive all-caps, tiny helper text, and too many font families.
- Ensure long technical values wrap or truncate safely.
- Use monospaced text for values such as paths, IDs, logs, or code only where it improves scanning.

## 4. Spacing and shape

- Establish a spacing scale and reuse it.
- Align labels, fields, buttons, and list columns.
- Use radii consistently by component role.
- Avoid nested containers without meaningful hierarchy.
- Cards are optional grouping tools, not a default wrapper for every section.

## 5. Motion and elevation

- Motion should explain a transition, acknowledge an action, or reveal hierarchy.
- Shadows/elevation should clarify layering, not decorate every surface.
- Gradients, blur, and texture are allowed when they support the approved design direction and remain legible.
- Avoid continuous decorative animation.

## 6. Component states

Document default, hover, focus-visible, pressed, selected, disabled, loading, and error states as relevant to each component.
