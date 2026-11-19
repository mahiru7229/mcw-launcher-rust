# Desktop Layout and Resizing

## Desktop-first assumptions

Confirm the actual supported platforms, minimum window size, display scaling, and input modes before setting layout constraints.

## Layout rules

- Optimize for common laptop and desktop window sizes, not only a maximized screenshot.
- Preserve a clear content hierarchy as width changes.
- Reflow sidebars, split panes, and detail panels deliberately.
- Avoid controls that become unreachable at smaller supported sizes.
- Keep primary actions in stable, discoverable locations.
- Do not stretch short content across the entire window merely to fill space.

## Density and window states

Review:
- Minimum supported window size.
- Typical non-maximized size.
- Maximized size.
- High display scaling.
- Long localized text.
- Large instance collections.
- Empty and error states.

## Responsive behavior

Even desktop applications need responsive layout behavior. Define breakpoints or adaptive rules based on available content space, not arbitrary device labels. If a panel must collapse, provide an obvious way to restore it.

## Performance

Avoid expensive re-layout, oversized image assets, and unnecessary animation during resize. Verify the framework's rendering and state update model.
