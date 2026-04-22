# OpenClaw Patches

This directory contains patches that will be applied to the OpenClaw source code during the build process.

## How to create patches

1. Make changes to the OpenClaw source code in `../openclaw`
2. Create a patch file:
   ```bash
   cd ../openclaw
   git diff > ../telegram-tt/patches/001-your-patch-name.patch
   ```

## Patch naming convention

- Use numeric prefixes to control application order: `001-`, `002-`, etc.
- Use descriptive names: `001-telegram-integration.patch`
- Patches are applied in alphabetical order

## Current patches

- None yet - add patches as needed for Telegram integration