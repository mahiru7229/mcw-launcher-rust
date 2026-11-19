# MCW Launcher Rust Edition — Product Design Direction

Status: **DRAFT / OWNER REVIEW REQUIRED**

This file defines MCW's visual identity. The agent must not treat generic design trends as approved brand decisions. Complete the TODO sections with product-owner decisions and visual references.

## 1. Product character

MCW Launcher should feel:
- **Purpose-built:** a desktop tool for managing and launching game instances, not a generic analytics dashboard.
- **Technical but approachable:** precise information without intimidating jargon where simpler wording works.
- **Efficient:** frequent tasks are easy to scan and execute.
- **Trustworthy:** progress, errors, and destructive consequences are communicated honestly.
- **Distinctive:** a recognizable identity built from a few consistent choices rather than decoration everywhere.

## 2. Visual direction — decisions to confirm

- Overall direction: TODO(OWNER) — e.g. restrained modern desktop utility, pixel-art influence, or another deliberate direction.
- Primary accent: TODO(OWNER)
- Neutral/background palette: TODO(OWNER)
- Typography family and fallback: TODO(OWNER)
- Icon family: TODO(OWNER)
- Shape language: TODO(OWNER)
- Illustration/mascot policy: TODO(OWNER)
- Light/dark theme support: TODO(OWNER)
- Reference screenshots or products: TODO(OWNER)

Do not infer these values from the product name or from the previous Python edition.

## 3. Visual principles

- Use hierarchy before decoration.
- Let instance identity, version, status, and primary action be easy to recognize.
- Use color semantically and consistently.
- Make technical detail available without forcing it into every user's primary path.
- Prefer a coherent visual rhythm over individually flashy components.
- Use expressive elements selectively where they strengthen product identity.
- A little warmth and personality is welcome; visual noise is not.

## 4. Desktop-first behavior

- Optimize primary workflows for desktop pointer and keyboard use.
- Keep important actions visible at common laptop window sizes.
- Reflow gracefully when the window is resized or display scaling changes.
- Use compact, scannable layouts for instance management and more spacious layouts for focused setup tasks.
- Avoid mobile-style oversized controls unless the framework or touch use case requires them.

## 5. Signature moments

Choose a small number of places where MCW can express more personality:
- TODO(OWNER): e.g. instance artwork or instance identity.
- TODO(OWNER): e.g. launch/ready state transition.
- TODO(OWNER): e.g. first-run experience.

Do not spread decorative motifs across every surface.

## 6. What MCW should avoid

- Generic SaaS dashboard styling unrelated to launcher workflows.
- Trend-driven glass/blur/gradient effects without a role.
- Excessive cards, pills, badges, and status colors.
- Decorative statistics or progress indicators.
- Random mixtures of icon styles and emoji.
- Huge empty hero areas that push useful controls below the fold.

## 7. Approval rule

Changes to foundational visual identity — palette, typography, icon system, navigation model, or global component language — require product-owner review before being treated as the new standard.
