# UX Flows — Launcher Workflows

This is a checklist, not an assertion that all listed features exist. Verify the Rust Edition's actual feature set before designing.

## First launch

Design for first run, setup in progress, complete, missing prerequisite, and failure/recovery. Explain required setup without front-loading optional preferences.

## Instance management

Users should be able to understand:
- Which instance is selected.
- Its version/configuration and meaningful status.
- Whether it is ready to launch.
- Where it is stored, if relevant.
- What the next available actions are.

Keep routine actions discoverable and advanced maintenance separate.

## Create/configure instance

- Use progressive disclosure.
- Explain loader/runtime/version terminology where needed.
- Validate before committing.
- Show real progress for long operations.
- Make cancellation behavior explicit.
- Never silently overwrite an existing instance.

## Download/repair/update

Represent actual phases where supported: checking, downloading, verifying, extracting, complete, failed, cancelled. Do not fabricate percentages. Explain retry and partial-data behavior.

## Launch/runtime

Represent real states only. Make launch action clear, provide useful failure recovery, and avoid freezing the UI during process work.

## Settings

Group by user goal. Explain advanced options. Make save/reset behavior clear. Avoid duplicating the same setting across pages.

## Long-running operation state model

Where applicable, consider idle, queued, running, paused, cancelling, succeeded, failed, and cancelled. Only expose states the implementation genuinely supports.
