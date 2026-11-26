# Windows Code Signing & Microsoft SmartScreen Verification Guide

Windows Defender SmartScreen shows an **"Unknown Publisher"** prompt for unsigned binaries until they are Authenticode-signed with a trusted Certificate Authority and establish download reputation.

## 1. Free Open-Source Authenticode Signing via SignPath Foundation

Because **MCW Launcher: Rust Edition** is open-source (MIT License, public GitHub repository, reproducible GitHub Actions releases), it qualifies for the **SignPath Foundation** free code-signing program:

1. Apply at <https://about.signpath.io/product/open-source> with `https://github.com/mahiru7229/mcw-launcher-rust` (`MIT` license).
2. Create an Organization, Project (`mcw-launcher-rust`), and Artifact Configuration / Signing Policy in SignPath, connecting the **GitHub Actions** integration.
3. Add these GitHub **Repository Secrets** (`Settings -> Secrets and variables -> Actions -> Secrets`):
   - `SIGNPATH_API_TOKEN`
4. Add these GitHub **Repository Variables** (`Settings -> Secrets and variables -> Actions -> Variables`):
   - `SIGNPATH_ENABLED` = `true`
   - `SIGNPATH_ORGANIZATION_ID` = `<your SignPath org UUID>`
   - `SIGNPATH_PROJECT_SLUG` = `mcw-launcher-rust`
   - `SIGNPATH_SIGNING_POLICY_SLUG` = `release-signing`

When `SIGNPATH_ENABLED` is `true`, `.github/workflows/release.yml` automatically submits `mcw-launcher-windows-x64.zip` through SignPath and cryptographically verifies via PowerShell `Get-AuthenticodeSignature` that both **`MCW-Launcher.exe`** and **`mcw-updater.exe`** carry valid Authenticode signatures before uploading to the GitHub Release.

Configure your SignPath **Artifact Configuration** XML to sign all Windows executables inside `mcw-launcher-windows-x64.zip`:

```xml
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file path="MCW-Launcher.exe" />
    <pe-file path="mcw-updater.exe" />
    <pe-file path="mcw-cli.exe" />
  </zip-file>
</artifact-configuration>
```

## 2. Submitting Releases to Microsoft Security Intelligence (SmartScreen Reputation)

Even with a valid Authenticode signature (including SignPath or Trusted Signing), freshly built release binaries accumulate SmartScreen trust faster when submitted directly to Microsoft:

1. Download the released `mcw-launcher-windows-x64.zip` (and note its `SHA256SUMS.txt`).
2. Visit Microsoft's **Software Developer Submission Portal**: <https://www.microsoft.com/en-us/wdsi/filesubmission>
3. Sign in with a Microsoft account, choose **Software developer**, select **Microsoft Defender SmartScreen**, and upload both **`MCW-Launcher.exe`** and **`mcw-updater.exe`** with clean project details pointing to the GitHub Release URL.
4. Keep `VERSIONINFO` metadata (`ProductName`, `CompanyName`, `FileDescription`, `LegalCopyright`) consistent across releases (configured in `apps/desktop/build.rs` and `apps/updater/build.rs`).
