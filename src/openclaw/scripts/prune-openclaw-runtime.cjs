#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const openclawDir = path.resolve('../openclaw');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found.');
  process.exit(1);
}

console.log('Pruning OpenClaw runtime...');

// List of directories/files to remove to reduce size
const pruneTargets = [
  'node_modules/.cache',
  'src',
  'tests',
  '*.test.js',
  '*.test.ts',
  'coverage',
  '.git',
  'docs',
  'examples',
  'README.md',
  'CHANGELOG.md'
];

// Change to openclaw directory
process.chdir(openclawDir);

for (const target of pruneTargets) {
  const targetPath = path.resolve(target);

  if (fs.existsSync(targetPath)) {
    try {
      const stats = fs.statSync(targetPath);
      if (stats.isDirectory()) {
        fs.rmSync(targetPath, { recursive: true, force: true });
        console.log(`Removed directory: ${target}`);
      } else {
        fs.unlinkSync(targetPath);
        console.log(`Removed file: ${target}`);
      }
    } catch (error) {
      console.warn(`Warning: Failed to remove ${target}:`, error.message);
    }
  }
}

console.log('Runtime pruning completed');