'use strict';

// Platform-selecting loader for the napi-rs native addon.
// The extension host requires this file as `../native`; the heavy .node binary
// lives next to it. No matching logic lives here — this is pure lookup.

const fs = require('fs');
const path = require('path');

function platformName() {
  switch (process.platform) {
    case 'linux': return 'linux';
    case 'darwin': return 'darwin';
    case 'win32': return 'win32';
    default: return process.platform;
  }
}

function archName() {
  switch (process.arch) {
    case 'x64': return 'x64';
    case 'arm64': return 'arm64';
    case 'ia32': return 'ia32';
    case 'arm': return 'arm';
    default: return process.arch;
  }
}

const plat = platformName();
const arch = archName();
const libc = fs.existsSync('/etc/alpine-release') ? 'musl' : 'gnu';

const candidates = [];
if (plat === 'linux') {
  candidates.push(`spring-nav.${plat}-${arch}-${libc}.node`);
}
if (plat === 'win32') {
  candidates.push(`spring-nav.${plat}-${arch}-msvc.node`);
}
candidates.push(`spring-nav.${plat}-${arch}.node`);
candidates.push('spring-nav.node');
candidates.push('index.node');

let loaded = false;
for (const name of candidates) {
  const full = path.join(__dirname, name);
  if (fs.existsSync(full)) {
    module.exports = require(full);
    loaded = true;
    break;
  }
}

if (!loaded) {
  throw new Error(
    'SpringNav: native module not found. Expected one of [' + candidates.join(', ') +
    '] in ' + __dirname + '. Run `npm run build:native`, or install a VSIX built for this platform.'
  );
}
