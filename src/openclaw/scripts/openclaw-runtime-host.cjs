#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const openclawDir = path.resolve('../openclaw');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

console.log('Starting OpenClaw runtime host...');

// Change to openclaw directory
process.chdir(openclawDir);

// Start the runtime host
const host = spawn('npm', ['run', 'start:host'], {
  stdio: 'inherit',
  shell: true
});

host.on('close', (code) => {
  console.log(`OpenClaw runtime host exited with code ${code}`);
});

host.on('error', (error) => {
  console.error('Failed to start OpenClaw runtime host:', error.message);
  process.exit(1);
});

// Handle graceful shutdown
process.on('SIGINT', () => {
  console.log('Shutting down OpenClaw runtime host...');
  host.kill('SIGINT');
});

process.on('SIGTERM', () => {
  console.log('Shutting down OpenClaw runtime host...');
  host.kill('SIGTERM');
});