/**
 * Logger module
 * Intercepts console.* methods and writes to file + console simultaneously.
 */

import path from 'path';
import fs from 'fs';
import * as os from 'os';

const LOG_RETENTION_DAYS = 7;
const LOG_MAX_SIZE = 80 * 1024 * 1024; // 80 MB

let _logDir: string | undefined;

function todayStr(): string {
  return new Date().toISOString().slice(0, 10); // YYYY-MM-DD
}

function getLogDir(): string {
  if (_logDir) return _logDir;
  
  const home = os.homedir();
  if (process.platform === 'darwin') {
    _logDir = path.join(home, 'Library', 'Logs', 'LobsterAI');
  } else if (process.platform === 'win32') {
    _logDir = path.join(process.env.APPDATA || path.join(home, 'AppData', 'Roaming'), 'LobsterAI', 'logs');
  } else {
    _logDir = path.join(home, '.config', 'LobsterAI', 'logs');
  }
  
  if (!fs.existsSync(_logDir)) {
    fs.mkdirSync(_logDir, { recursive: true });
  }
  return _logDir;
}

function getLogPath(): string {
  return path.join(getLogDir(), `main-${todayStr()}.log`);
}

function formatMessage(level: string, ...args: any[]): string {
  const now = new Date();
  const timestamp = `[${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')} ${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}.${String(now.getMilliseconds()).padStart(3, '0')}]`;
  
  const text = args.map(arg => {
    if (arg instanceof Error) {
      return arg.stack || arg.message;
    }
    if (typeof arg === 'object') {
      try {
        return JSON.stringify(arg, null, 2);
      } catch {
        return String(arg);
      }
    }
    return String(arg);
  }).join(' ');

  return `${timestamp} [${level.toUpperCase()}] ${text}\n`;
}

function writeToFile(message: string): void {
  try {
    const logPath = getLogPath();
    // Simple rotation check
    if (fs.existsSync(logPath) && fs.statSync(logPath).size > LOG_MAX_SIZE) {
      fs.renameSync(logPath, `${logPath}.old`);
    }
    fs.appendFileSync(logPath, message, 'utf8');
  } catch (error) {
    process.stderr.write(`Failed to write to log file: ${error}\n`);
  }
}

export function initLogger(): void {
  const originalLog = console.log;
  const originalError = console.error;
  const originalWarn = console.warn;
  const originalInfo = console.info;
  const originalDebug = console.debug;

  console.log = (...args: any[]) => {
    originalLog.apply(console, args);
    writeToFile(formatMessage('info', ...args));
  };
  console.error = (...args: any[]) => {
    originalError.apply(console, args);
    writeToFile(formatMessage('error', ...args));
  };
  console.warn = (...args: any[]) => {
    originalWarn.apply(console, args);
    writeToFile(formatMessage('warn', ...args));
  };
  console.info = (...args: any[]) => {
    originalInfo.apply(console, args);
    writeToFile(formatMessage('info', ...args));
  };
  console.debug = (...args: any[]) => {
    originalDebug.apply(console, args);
    writeToFile(formatMessage('debug', ...args));
  };

  pruneOldLogs();

  console.info('='.repeat(60));
  console.info(`LobsterAI started (${process.platform} ${process.arch})`);
  console.info('='.repeat(60));
}

function pruneOldLogs(): void {
  try {
    const dir = getLogDir();
    const files = fs.readdirSync(dir);
    const now = Date.now();
    const retentionMs = LOG_RETENTION_DAYS * 24 * 60 * 60 * 1000;

    for (const file of files) {
      if (!file.startsWith('main-') || !file.endsWith('.log')) continue;
      const filePath = path.join(dir, file);
      const stats = fs.statSync(filePath);
      if (now - stats.mtimeMs > retentionMs) {
        fs.unlinkSync(filePath);
      }
    }
  } catch (error) {
    process.stderr.write(`Failed to prune old logs: ${error}\n`);
  }
}

export function getLogsDir(): string {
  return getLogDir();
}
