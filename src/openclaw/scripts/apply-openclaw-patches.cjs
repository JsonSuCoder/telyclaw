#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const patchesDir = path.resolve('./src/openclaw/patches');
const openclawDir = path.resolve('../openclaw');

console.log('Applying OpenClaw patches...');

if (!fs.existsSync(patchesDir)) {
  console.log('No patches directory found, skipping patches');
  process.exit(0);
}

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

// Change to openclaw directory
process.chdir(openclawDir);

// Find all patch files
const patchFiles = fs.readdirSync(patchesDir)
  .filter(file => file.endsWith('.patch'))
  .sort();

if (patchFiles.length === 0) {
  console.log('No patch files found');
  process.exit(0);
}

// Apply each patch
for (const patchFile of patchFiles) {
  const patchPath = path.join(patchesDir, patchFile);
  console.log(`Applying patch: ${patchFile}`);

  try {
    execSync(`git apply --ignore-whitespace "${patchPath}"`, { stdio: 'inherit' });
    console.log(`Successfully applied ${patchFile}`);
  } catch (error) {
    console.warn(`Warning: Failed to apply patch ${patchFile}:`, error.message);
    // Continue with other patches
  }
}

console.log('Patch application completed');