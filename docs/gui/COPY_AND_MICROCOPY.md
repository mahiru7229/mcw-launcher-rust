# UI Copy and Microcopy

## Principles

- Use concise, direct language.
- Prefer concrete verbs and specific action labels.
- Explain consequences before destructive actions.
- Avoid generic enthusiasm, marketing filler, and empty slogans.
- Do not invent product capabilities or imply guarantees the software cannot make.
- Use consistent terminology for instances, versions, loaders, repair, launch, and updates.

## Errors

A useful error message answers:
1. What failed?
2. What does it affect?
3. Is user data safe?
4. What can the user do next?

Show technical detail through diagnostics when needed, not as an unexplained wall of text.

## Progress

- Report actual phases or measurable progress only.
- If exact progress is unavailable, use an honest indeterminate state.
- Distinguish "queued", "running", "verifying", and "complete" only when the application knows those states.
- Avoid fake precision.

## Labels

Prefer "Create instance" over "New". Prefer "Repair files" over "Fix". Keep labels consistent across screens and documentation.

## Localization

If the project has localization infrastructure, do not hardcode new user-facing strings into presentation logic. Allow for longer translations and avoid concatenating sentence fragments in ways that break grammar.
