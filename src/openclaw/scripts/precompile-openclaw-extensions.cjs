#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const openclawDir = path.resolve('../openclaw');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

console.log('Precompiling OpenClaw extensions...');

// Change to openclaw directory
process.chdir(openclawDir);

try {
  // Precompile extensions
  execSync('npm run precompile:extensions', { stdio: 'inherit' });
  console.log('Extensions precompilation completed');
} catch (error) {
  console.warn('Warning: Failed to precompile extensions:', error.message);
  // Don't exit with error as this might be optional
}