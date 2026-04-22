#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const openclawDir = path.resolve('../openclaw');
const localExtensionsDir = path.resolve('./src/openclaw/openclaw-extensions');
const targetExtensionsDir = path.join(openclawDir, 'openclaw-extensions');

console.log('Syncing local OpenClaw extensions...');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

if (!fs.existsSync(localExtensionsDir)) {
  console.log('No local extensions found, skipping sync');
  process.exit(0);
}

// Create target extensions directory if it doesn't exist
if (!fs.existsSync(targetExtensionsDir)) {
  fs.mkdirSync(targetExtensionsDir, { recursive: true });
}

// Copy local extensions to openclaw directory
fs.cpSync(localExtensionsDir, targetExtensionsDir, {
  recursive: true,
  force: true
});

console.log(`Local extensions synced to ${targetExtensionsDir}`);