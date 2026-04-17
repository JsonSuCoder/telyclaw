import * as path from 'path';
import * as os from 'os';

export function getUserDataPath(): string {
  if (process.env.OPENCLAW_DATA_PATH) {
    return process.env.OPENCLAW_DATA_PATH;
  }
  // Fallback to home directory if not provided
  return path.join(os.homedir(), '.openclaw');
}

export function getResourcesPath(): string {
  if (process.env.OPENCLAW_RESOURCES_PATH) {
    return process.env.OPENCLAW_RESOURCES_PATH;
  }
  // In development or if not provided, try to find it relative to current working directory
  return path.join(process.cwd(), 'resources');
}

export function isPackaged(): boolean {
  return process.env.NODE_ENV === 'production' || !!process.env.OPENCLAW_PACKAGED;
}

export function getAppName(): string {
  return process.env.OPENCLAW_APP_NAME || 'OpenClaw';
}

export function getAppVersion(): string {
  return process.env.OPENCLAW_APP_VERSION || '1.0.0';
}
