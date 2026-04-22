#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const platform = process.argv[2];
if (!platform) {
  console.error('Usage: node sync-openclaw-runtime-current.cjs <platform>');
  process.exit(1);
}

const openclawDir = path.resolve('../openclaw');
const targetDir = path.resolve('./src/openclaw');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found.');
  process.exit(1);
}

console.log(`Syncing OpenClaw runtime for ${platform}...`);

// Create target directory if it doesn't exist
if (!fs.existsSync(targetDir)) {
  fs.mkdirSync(targetDir, { recursive: true });
}

// Copy built runtime files
const runtimeSrcDir = path.join(openclawDir, 'dist', platform);
const runtimeTargetDir = path.join(targetDir, 'runtime');

if (fs.existsSync(runtimeSrcDir)) {
  // Remove existing runtime
  if (fs.existsSync(runtimeTargetDir)) {
    fs.rmSync(runtimeTargetDir, { recursive: true, force: true });
  }

  // Copy new runtime
  fs.cpSync(runtimeSrcDir, runtimeTargetDir, { recursive: true });
  console.log(`Runtime synced to ${runtimeTargetDir}`);
} else {
  console.warn(`Runtime directory not found: ${runtimeSrcDir}`);
}

console.log('Runtime sync completed');