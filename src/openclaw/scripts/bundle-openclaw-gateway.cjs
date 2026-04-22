#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const openclawDir = path.resolve('../openclaw');
const targetDir = path.resolve('./src/openclaw');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

console.log('Bundling OpenClaw gateway...');

// Change to openclaw directory
process.chdir(openclawDir);

try {
  // Check if gateway already exists in dist
  const gatewaySrcDir = path.join(openclawDir, 'dist', 'gateway');
  const gatewayTargetDir = path.join(targetDir, 'gateway');

  if (fs.existsSync(gatewaySrcDir)) {
    // Remove existing gateway
    if (fs.existsSync(gatewayTargetDir)) {
      fs.rmSync(gatewayTargetDir, { recursive: true, force: true });
    }

    // Copy gateway from dist
    fs.cpSync(gatewaySrcDir, gatewayTargetDir, { recursive: true });
    console.log(`Gateway bundled to ${gatewayTargetDir}`);
  } else {
    console.log('Gateway not found in dist, skipping gateway bundling');
    console.log('Note: Gateway will be available after OpenClaw runtime build');
  }

  console.log('Gateway bundling completed');
} catch (error) {
  console.error('Failed to bundle gateway:', error.message);
  process.exit(1);
}