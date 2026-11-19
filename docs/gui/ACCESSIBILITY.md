# Accessibility Guidelines

## Keyboard and focus

- Primary workflows must be keyboard reachable.
- Focus must be visible and follow a logical order.
- Do not make hover the only way to discover an action.
- Keep focus within modal interactions where appropriate and restore it on close.
- Use Enter/Escape only where behavior is predictable.

## Contrast and state

- Verify text and control contrast.
- Do not communicate status by color alone.
- Check selected, disabled, error, hover, and focus states.
- Test both supported themes.

## Scaling and content

- Test common desktop display scaling and realistic window sizes.
- Prevent clipping of long names, paths, versions, and localized strings.
- Ensure dialogs fit the available screen.
- Avoid fixed dimensions that break ordinary resizing.

## Semantics

Use framework accessibility APIs where available: meaningful names, roles, state, reading order, and announced validation/progress. Document limitations instead of claiming untested compliance.

## Motion

- Avoid motion that is continuous or required to understand information.
- Respect reduced-motion preferences where supported.
- Ensure state changes remain understandable without animation.
