# GUI Design Principles

## 1. Design from user jobs

For each screen, define the user, task, primary action, required information, and likely failure path. Do not start with a component gallery or a fashionable layout.

## 2. Information hierarchy

A useful default order:
1. Current context — selected instance, page, or operation.
2. Information needed to decide what to do.
3. Primary action.
4. Secondary actions and supporting detail.
5. Advanced settings and diagnostics.

Adjust this order when user research or the workflow indicates otherwise.

## 3. Desktop layout

- Align related elements to a consistent grid.
- Group by task and meaning, not by decoration.
- Keep high-frequency actions visible.
- Use whitespace to separate meaningful groups.
- Avoid unnecessary horizontal scrolling.
- Preserve selection and navigation context when reasonable.
- Use minimum window sizes only when needed to protect usability.

## 4. Density

- Instance lists and management views should support scanning.
- Setup flows should explain unfamiliar choices.
- Advanced settings can be denser but need clear grouping.
- Do not force every screen into the same density.

## 5. Product specificity

Use MCW terminology, actual instance data, real operation states, and workflow-specific affordances. Do not add generic cards, charts, welcome banners, or dashboard widgets unless they serve a real launcher task.

## 6. Visual restraint and expression

Color, borders, elevation, motion, icons, and illustrations are all valid tools. Each should communicate hierarchy, state, identity, or feedback. Consistent personality is better than constant decoration.
