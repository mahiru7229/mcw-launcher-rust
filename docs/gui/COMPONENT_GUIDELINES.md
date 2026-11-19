# Component Guidelines

## Buttons and actions

- Use clear verb labels: "Launch", "Create instance", "Repair".
- Establish primary, secondary, quiet, and destructive roles only where needed.
- Avoid multiple competing primary actions in one local task area.
- Disabled actions should explain why when the reason is not obvious.
- Destructive actions must communicate consequences.

## Forms

- Keep labels visible and consistent.
- Explain units, constraints, and consequences.
- Validate at a useful time and preserve input after recoverable errors.
- Separate common options from advanced configuration.
- Distinguish placeholders from actual values.

## Lists and tables

- Optimize for scanning with stable alignment and clear selection.
- Keep item identity and meaningful status visible.
- Put infrequent actions in menus, but do not hide essential actions exclusively there.
- Design empty, loading, and failure states.
- Avoid columns that add noise without helping a decision.

## Dialogs

- Use dialogs for focused decisions or bounded tasks.
- State consequences plainly and use specific action labels.
- Avoid stacked dialogs.
- Do not ask confirmation for harmless, reversible actions.
- Identify what is affected by destructive operations.

## Navigation

- Use stable labels and predictable hierarchy.
- Show current location.
- Preserve context when moving between list, detail, and settings.
- Avoid competing navigation patterns.

## Notifications and errors

- Distinguish transient feedback from persistent issues.
- Errors should say what happened, what the user can do, and whether data is safe.
- Keep stack traces and diagnostics out of ordinary user-facing copy; provide a diagnostics path when useful.
- Do not report completion before the operation and verification finish.

## Icons

- Use one coherent icon family or a deliberately consistent custom set.
- Pair unfamiliar or important icon-only actions with labels.
- Do not mix emoji and unrelated icon styles.
