'use strict';
// End-to-end logic harness against a real SpringBoot project (default: RuoYi-Vue).
// Usage: node test/harness.js [project-root]
// It drives the real .node addon (no VS Code) and asserts the J-* / S-* / M-* / C-* / F-* chains.

const fs = require('fs');
const path = require('path');
const addon = require('../native');

const root = process.argv[2] || process.env.SPRINGNAV_ROOT || '/tmp/RuoYi-Vue';
if (!fs.existsSync(path.join(root, 'pom.xml'))) {
  console.error('not a Java project root:', root);
  process.exit(1);
}

const P = {
  ctrl: 'ruoyi-admin/src/main/java/com/ruoyi/web/controller/system/SysUserController.java',
  svc: 'ruoyi-system/src/main/java/com/ruoyi/system/service/ISysUserService.java',
  impl: 'ruoyi-system/src/main/java/com/ruoyi/system/service/impl/SysUserServiceImpl.java',
  mapper: 'ruoyi-system/src/main/java/com/ruoyi/system/mapper/SysUserMapper.java',
  xml: 'ruoyi-system/src/main/resources/mapper/system/SysUserMapper.xml',
  entity: 'ruoyi-common/src/main/java/com/ruoyi/common/core/domain/entity/SysUser.java',
};

function pos(rel, needle, offset = 0, occurrence = 0) {
  const lines = fs.readFileSync(path.join(root, rel), 'utf8').split('\n');
  let seen = 0;
  for (let i = 0; i < lines.length; i++) {
    let from = 0;
    let idx;
    while ((idx = lines[i].indexOf(needle, from)) !== -1) {
      if (seen === occurrence) {
        return { line: i, character: idx + offset, text: lines[i] };
      }
      seen += 1;
      from = idx + 1;
    }
  }
  throw new Error(`needle not found: "${needle}" in ${rel} (occurrence ${occurrence})`);
}

function resolve(rel, needle, offset = 0, occurrence = 0) {
  const p = pos(rel, needle, offset, occurrence);
  const uri = addon.uriForPath(path.join(root, rel));
  return addon.resolveDefinition(uri, p.line, p.character, p.text);
}

function resolveDir(rel, needle, direction, offset = 0, occurrence = 0, fallback = true) {
  const p = pos(rel, needle, offset, occurrence);
  const uri = addon.uriForPath(path.join(root, rel));
  return addon.resolveDirectional(uri, p.line, p.character, p.text, direction, fallback);
}

function hasLine(out, rel, line) {
  return out.some((l) => l.uri.includes(rel) && l.line === line);
}

const results = [];
function check(name, fn) {
  try {
    const info = fn();
    console.log(`  PASS  ${name}${info ? '  ' + info : ''}`);
    results.push(true);
  } catch (e) {
    console.log(`  FAIL  ${name}: ${e.message}`);
    results.push(false);
  }
}

function has(out, rel) {
  return out.some((l) => l.uri.includes(rel));
}

(async () => {
  console.log('root:', root);
  console.log('isJavaProject:', addon.isJavaProject(root));
  const count = await addon.buildIndex(root);
  console.log('indexed files:', count);
  console.log('stats:', addon.indexStats());
  console.log('');

  check('J-01 Controller -> Service interface (RuoYi)', () => {
    const out = resolve(P.ctrl, 'selectUserList(user)', 0, 0);
    if (!has(out, P.svc)) throw new Error('no ISysUserService location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('J-02 Service interface -> ServiceImpl', () => {
    const out = resolve(P.svc, 'selectUserList(SysUser user)');
    if (!has(out, P.impl)) throw new Error('no impl location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('J-05 reverse: Service interface -> Controller', () => {
    const out = resolve(P.svc, 'selectUserList(SysUser user)');
    if (!has(out, P.ctrl)) throw new Error('no controller location: ' + JSON.stringify(out.slice(0, 4)));
    return 'ok';
  });

  check('J-03 ServiceImpl -> Mapper', () => {
    const out = resolve(P.impl, 'selectUserList(user)', 0, 0);
    if (!has(out, P.mapper)) throw new Error('no mapper location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('J-04 Mapper -> XML statement', () => {
    const out = resolve(P.mapper, 'selectUserList(SysUser sysUser)');
    if (!has(out, P.xml)) throw new Error('no xml location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('J-05 reverse: XML -> Mapper', () => {
    const out = resolve(P.xml, '<select id="selectUserList"');
    if (!has(out, P.mapper)) throw new Error('no mapper location: ' + JSON.stringify(out.slice(0, 4)));
    return 'ok';
  });

  check('J-06 entity field -> get/set (SysUser.userId)', () => {
    const out = resolve(P.entity, 'private Long userId;', 'private Long '.length);
    const inEntity = out.filter((l) => l.uri.includes(P.entity));
    if (inEntity.length < 2) throw new Error('expected getter+setter, got ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${inEntity.length} methods`;
  });

  check('J-06 reverse: getter -> field', () => {
    const out = resolve(P.entity, 'getUserId()');
    if (!has(out, P.entity)) throw new Error('no entity location: ' + JSON.stringify(out.slice(0, 4)));
    return 'ok';
  });

  check('S-01 searchByPath /system/user/list -> Controller', () => {
    const out = addon.searchByPath('/system/user/list');
    if (!has(out, P.ctrl)) throw new Error('no controller location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('S-01 path with variable /system/user/{userId}', () => {
    const out = addon.searchByPath('/system/user/{userId}');
    if (!has(out, P.ctrl)) throw new Error('no controller location: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length} location(s)`;
  });

  check('M-02 dedup + stable sort', () => {
    const out = resolve(P.svc, 'selectUserList(SysUser user)');
    const keys = out.map((l) => `${l.uri}:${l.line}:${l.character}`);
    if (new Set(keys).size !== keys.length) throw new Error('duplicates present');
    for (let i = 1; i < keys.length; i++) {
      if (keys[i - 1] > keys[i]) throw new Error('not sorted');
    }
    return 'ok';
  });

  check('D-01 down: Controller call -> Service interface', () => {
    const out = resolveDir(P.ctrl, 'selectUserList(user)', 'down', 0, 0);
    if (!has(out, P.svc)) throw new Error('no service: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length}`;
  });

  check('D-02 down: Service interface -> Impl', () => {
    const out = resolveDir(P.svc, 'selectUserList(SysUser user)', 'down');
    if (!has(out, P.impl)) throw new Error('no impl: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length}`;
  });

  check('D-03 up: Service interface -> Controller', () => {
    const out = resolveDir(P.svc, 'selectUserList(SysUser user)', 'up');
    if (!has(out, P.ctrl)) throw new Error('no controller: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length}`;
  });

  check('D-04 down: ServiceImpl -> Mapper', () => {
    const out = resolveDir(P.impl, 'selectUserList(user)', 'down', 0, 0);
    if (!has(out, P.mapper)) throw new Error('no mapper: ' + JSON.stringify(out.slice(0, 4)));
    return `-> ${out.length}`;
  });

  check('D-05 down: Mapper -> XML; up: XML -> Mapper', () => {
    const down = resolveDir(P.mapper, 'selectUserList(SysUser sysUser)', 'down');
    const up = resolveDir(P.xml, '<select id="selectUserList"', 'up');
    if (!has(down, P.xml)) throw new Error('down: no xml');
    if (!has(up, P.mapper)) throw new Error('up: no mapper');
    return 'ok';
  });

  check('D-06 fallback: XML has no down -> falls back to up (Mapper)', () => {
    const out = resolveDir(P.xml, '<select id="selectUserList"', 'down');
    if (!has(out, P.mapper)) throw new Error('no mapper fallback: ' + JSON.stringify(out.slice(0, 4)));
    return 'ok';
  });

  check('D-07 entity: getter down -> field; field up -> getter', () => {
    const f = pos(P.entity, 'private Long userId;', 'private Long '.length);
    const g = pos(P.entity, 'getUserId()');
    const down = resolveDir(P.entity, 'getUserId()', 'down');
    const up = resolveDir(P.entity, 'private Long userId;', 'up', 'private Long '.length);
    if (!hasLine(down, P.entity, f.line)) throw new Error('getter down did not reach field line ' + f.line);
    if (!hasLine(up, P.entity, g.line)) throw new Error('field up did not reach getter line ' + g.line);
    return 'ok';
  });

  check('D-08 path hits carry file/verb/module', () => {
    const hits = addon.searchPathHits('/system/user/list');
    if (hits.length === 0) throw new Error('no hits');
    const h = hits[0];
    if (h.path !== '/system/user/list') throw new Error('bad path ' + h.path);
    if (!h.file.includes('SysUserController.java')) throw new Error('bad file ' + h.file);
    if (h.method !== 'GET') throw new Error('bad method ' + h.method);
    if (!h.module.includes('ruoyi-admin')) throw new Error('bad module ' + h.module);
    return `${h.path} ${h.file} [${h.method}] ${h.module}`;
  });

  check('D-09 multi-token real-time hits', () => {
    const hits = addon.searchPathHits('system user list');
    if (!hits.some((h) => h.path === '/system/user/list')) {
      throw new Error('no multi-token hit: ' + JSON.stringify(hits.slice(0, 3)));
    }
    return `-> ${hits.length}`;
  });

  // C-02/C-03/C-04: an explicit alias overrides the inferred field type.
  const cfgDir = path.join(root, '.idea');
  const cfgPath = path.join(cfgDir, 'spring-nav.toml');
  const hadCfg = fs.existsSync(cfgPath);
  const saved = hadCfg ? fs.readFileSync(cfgPath) : null;
  try {
    fs.mkdirSync(cfgDir, { recursive: true });
    fs.writeFileSync(cfgPath, '[aliases]\n"userService" = "NoSuchTypeAnywhere"\n');
    addon.reloadConfig();
    check('C-03 alias overrides naming/type inference', () => {
      const out = resolve(P.ctrl, 'selectUserList(user)', 0, 0);
      if (has(out, P.svc)) throw new Error('alias did not override field type');
      return 'ok';
    });
  } finally {
    if (hadCfg) {
      fs.writeFileSync(cfgPath, saved);
    } else {
      fs.rmSync(cfgPath, { force: true });
      try { fs.rmdirSync(cfgDir); } catch (_) {}
    }
    addon.reloadConfig();
  }

  check('C-04 config reload restores default resolution', () => {
    const out = resolve(P.ctrl, 'selectUserList(user)', 0, 0);
    if (!has(out, P.svc)) throw new Error('default resolution not restored');
    return 'ok';
  });

  check('F-03 updateFile re-indexes one saved file', () => {
    addon.updateFile(addon.uriForPath(path.join(root, P.ctrl)));
    const out = resolve(P.ctrl, 'selectUserList(user)', 0, 0);
    if (!has(out, P.svc)) throw new Error('updateFile broke the index');
    return 'ok';
  });

  const failed = results.filter((r) => !r).length;
  console.log(`\n${results.length - failed}/${results.length} checks passed`);
  process.exit(failed ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});
