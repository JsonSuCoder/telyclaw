#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const packageJson = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const openclawConfig = packageJson.openclaw;

if (!openclawConfig || !openclawConfig.plugins) {
  console.log('No OpenClaw plugins configured');
  process.exit(0);
}

const openclawDir = path.resolve('../openclaw');
const pluginsDir = path.join(openclawDir, 'plugins');

if (!fs.existsSync(openclawDir)) {
  console.error('OpenClaw directory not found. Run openclaw:ensure first.');
  process.exit(1);
}

console.log('Installing OpenClaw plugins...');

// Create plugins directory if it doesn't exist
if (!fs.existsSync(pluginsDir)) {
  fs.mkdirSync(pluginsDir, { recursive: true });
}

// Change to plugins directory
process.chdir(pluginsDir);

// Install each plugin
for (const plugin of openclawConfig.plugins) {
  const { id, npm, version, registry, optional } = plugin;

  console.log(`Installing plugin: ${id} (${npm}@${version})`);

  try {
    let installCmd = `npm install ${npm}@${version} --legacy-peer-deps`;
    if (registry) {
      installCmd += ` --registry=${registry}`;
    }

    try {
      execSync(installCmd, { stdio: 'inherit' });
    } catch (installError) {
      console.warn(`Retrying with --force for plugin ${id}...`);
      installCmd = installCmd.replace('--legacy-peer-deps', '--force');
      execSync(installCmd, { stdio: 'inherit' });
    }
    console.log(`Successfully installed ${id}`);
  } catch (error) {
    if (optional) {
      console.warn(`Warning: Failed to install optional plugin ${id}:`, error.message);
    } else {
      console.error(`Failed to install required plugin ${id}:`, error.message);
      process.exit(1);
    }
  }
}

console.log('Plugin installation completed');