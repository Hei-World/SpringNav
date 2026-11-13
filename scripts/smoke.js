'use strict';
// Self-contained logic check: creates a tiny SpringBoot+MyBatis project,
// builds the index through the real .node addon and asserts every jump chain.
// No VS Code / GUI required.

const fs = require('fs');
const path = require('path');
const os = require('os');
const addon = require('../native');

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'springnav-fixture-'));

function write(rel, content) {
  const p = path.join(root, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, content);
}

write('pom.xml', '<project><modelVersion>4.0.0</modelVersion></project>\n');

write(
  'src/main/java/com/example/controller/UserController.java',
  `package com.example.controller;

import com.example.service.UserService;
import com.example.domain.User;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/v1/user")
public class UserController {
    private UserService userService;

    @GetMapping("/getInfo")
    public User getInfo(Long id) {
        return userService.getInfo(id);
    }

    @PostMapping("/save")
    public void save(User user) {
        userService.save(user);
    }

    public void ping() {}

    public void ref() {
        Runnable r = this::ping;
    }
}
`
);

write(
  'src/main/java/com/example/service/UserService.java',
  `package com.example.service;

import com.example.domain.User;

public interface UserService {
    User getInfo(Long id);

    User getInfo(String name);

    void save(User user);
}
`
);

write(
  'src/main/java/com/example/service/impl/UserServiceImpl.java',
  `package com.example.service.impl;

import com.example.service.UserService;
import com.example.mapper.UserMapper;
import com.example.domain.User;
import org.springframework.stereotype.Service;

@Service
public class UserServiceImpl implements UserService {
    private UserMapper userMapper;

    @Override
    public User getInfo(Long id) {
        return userMapper.selectById(id);
    }

    @Override
    public User getInfo(String name) {
        return userMapper.selectByName(name);
    }

    @Override
    public void save(User user) {
        userMapper.insert(user);
    }
}
`
);

write(
  'src/main/java/com/example/mapper/BaseMapper.java',
  `package com.example.mapper;

import com.example.domain.User;

public interface BaseMapper {
    User selectByName(String name);
}
`
);

write(
  'src/main/java/com/example/mapper/UserMapper.java',
  `package com.example.mapper;

import com.example.domain.User;

public interface UserMapper extends BaseMapper {
    User selectById(Long id);

    int insert(User user);
}
`
);

write(
  'src/main/resources/mapper/UserMapper.xml',
  `<?xml version="1.0" encoding="UTF-8" ?>
<mapper namespace="com.example.mapper.UserMapper">
    <select id="selectById" resultType="com.example.domain.User">
        select * from t_user where id = #{id}
    </select>
    <select id="selectByName" resultType="com.example.domain.User">
        select * from t_user where name = #{name}
    </select>
    <insert id="insert">
        insert into t_user (id, name) values (#{id}, #{name})
    </insert>
</mapper>
`
);

write(
  'src/main/java/com/example/domain/User.java',
  `package com.example.domain;

public class User {
    private Long id;
    private String name;

    public Long getId() { return id; }

    public void setId(Long id) { this.id = id; }

    public String getName() { return name; }

    public void setName(String name) { this.name = name; }
}
`
);

// Lombok: getters/setters are generated at compile time, not present in source.
write(
  'src/main/java/com/example/domain/UserDto.java',
  `package com.example.domain;

import lombok.Data;

@Data
public class UserDto {
    private Long id;
    private String name;
    private boolean enabled;
    private static final long serialVersionUID = 1L;
    private final String code = "x";

    public Long getId() { return id; }
}
`
);

write(
  'src/main/java/com/example/controller/DtoController.java',
  `package com.example.controller;

import com.example.domain.UserDto;

public class DtoController {
    private UserDto userDto;

    public void run() {
        userDto.getId();
        userDto.getName();
        userDto.setName("n");
        userDto.isEnabled();
        userDto.getSerialVersionUID();
        userDto.setCode("c");
    }
}
`
);

// Position of the first occurrence of `needle` (+ offset) in a file.
function pos(rel, needle, offset = 0) {
  const lines = fs.readFileSync(path.join(root, rel), 'utf8').split('\n');
  for (let i = 0; i < lines.length; i++) {
    const idx = lines[i].indexOf(needle);
    if (idx !== -1) {
      return { line: i, character: idx + offset, text: lines[i] };
    }
  }
  throw new Error(`needle not found: ${needle} in ${rel}`);
}

function resolve(rel, needle, offset = 0) {
  const p = pos(rel, needle, offset);
  const uri = addon.uriForPath(path.join(root, rel));
  const out = addon.resolveDefinition(uri, p.line, p.character, p.text);
  return { out, at: p };
}

function resolveDir(rel, needle, direction, offset = 0, fallback = true) {
  const p = pos(rel, needle, offset);
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

(async () => {
  console.log('isJavaProject:', addon.isJavaProject(root));
  const res = await addon.buildIndex([root]);
  console.log('indexed:', JSON.stringify(res));
  console.log('stats:', addon.indexStats());

  const base = 'src/main/java/com/example/';
  const svc = base + 'service/UserService.java';
  const impl = base + 'service/impl/UserServiceImpl.java';
  const mapper = base + 'mapper/UserMapper.java';
  const baseMapper = base + 'mapper/BaseMapper.java';
  const xml = 'src/main/resources/mapper/UserMapper.xml';
  const ctrl = base + 'controller/UserController.java';
  const entity = base + 'domain/User.java';
  const dto = base + 'domain/UserDto.java';
  const dtoCtrl = base + 'controller/DtoController.java';

  check('J-01 Controller call -> Service interface', () => {
    const { out } = resolve(ctrl, 'getInfo(id)');
    if (!out.some((l) => l.uri.includes(svc))) throw new Error('no service location: ' + JSON.stringify(out));
    return `-> ${out.length} location(s)`;
  });

  check('J-08 overloads produce a list', () => {
    const { out } = resolve(ctrl, 'getInfo(id)');
    const inSvc = out.filter((l) => l.uri.includes(svc));
    if (inSvc.length < 2) throw new Error('expected >=2 overloads, got ' + JSON.stringify(out));
    return `-> ${inSvc.length} overloads`;
  });

  check('J-02 Service interface -> Impl', () => {
    const { out } = resolve(svc, 'getInfo(Long id)');
    if (!out.some((l) => l.uri.includes(impl))) throw new Error('no impl location: ' + JSON.stringify(out));
    return `-> ${out.length} location(s)`;
  });

  check('J-05 reverse: Service interface -> Controller', () => {
    const { out } = resolve(svc, 'getInfo(Long id)');
    if (!out.some((l) => l.uri.includes(ctrl))) throw new Error('no controller location: ' + JSON.stringify(out));
    return 'ok';
  });

  check('J-03 ServiceImpl call -> Mapper', () => {
    const { out } = resolve(impl, 'selectById(id)');
    if (!out.some((l) => l.uri.includes(mapper))) throw new Error('no mapper location: ' + JSON.stringify(out));
    return `-> ${out.length} location(s)`;
  });

  check('J-04 Mapper method -> XML statement', () => {
    const { out } = resolve(mapper, 'selectById(Long id)');
    if (!out.some((l) => l.uri.includes(xml))) throw new Error('no xml location: ' + JSON.stringify(out));
    return `-> ${out.length} location(s)`;
  });

  check('J-05 reverse: XML -> Mapper', () => {
    const { out } = resolve(xml, '<select id="selectById"');
    if (!out.some((l) => l.uri.includes(mapper))) throw new Error('no mapper location: ' + JSON.stringify(out));
    return 'ok';
  });

  check('J-06 entity field -> get/set', () => {
    const { out } = resolve(entity, 'private Long id;', 'private Long '.length);
    const getters = out.filter((l) => l.uri.includes(entity));
    if (getters.length < 2) throw new Error('expected getter+setter, got ' + JSON.stringify(out));
    return `-> ${getters.length} methods`;
  });

  check('J-06 reverse: getter -> field', () => {
    const { out } = resolve(entity, 'getId()');
    if (!out.some((l) => l.uri.includes(entity))) throw new Error('no entity location: ' + JSON.stringify(out));
    return 'ok';
  });

  check('J-07 this::method -> method decl', () => {
    const { out } = resolve(ctrl, '::ping', 2);
    if (!out.some((l) => l.uri.includes(ctrl))) throw new Error('no ping location: ' + JSON.stringify(out));
    return 'ok';
  });

  check('J-09 inheritance: call resolves method from super-interface', () => {
    const { out } = resolve(impl, 'selectByName(name)');
    if (!out.some((l) => l.uri.includes(baseMapper))) {
      throw new Error('no BaseMapper location: ' + JSON.stringify(out));
    }
    return 'ok';
  });

  check('D-01 down: Controller call -> Service interface', () => {
    const out = resolveDir(ctrl, 'getInfo(id)', 'down');
    if (!out.some((l) => l.uri.includes(svc))) throw new Error('no service: ' + JSON.stringify(out));
    return `-> ${out.length}`;
  });

  check('D-02 down: Service interface -> Impl; up -> Controller', () => {
    const down = resolveDir(svc, 'getInfo(Long id)', 'down');
    const up = resolveDir(svc, 'getInfo(Long id)', 'up');
    if (!down.some((l) => l.uri.includes(impl))) throw new Error('down: no impl');
    if (!up.some((l) => l.uri.includes(ctrl))) throw new Error('up: no controller');
    return 'ok';
  });

  check('D-03 down: Impl call -> Mapper', () => {
    const out = resolveDir(impl, 'selectById(id)', 'down');
    if (!out.some((l) => l.uri.includes(mapper))) throw new Error('no mapper: ' + JSON.stringify(out));
    return `-> ${out.length}`;
  });

  check('D-04 down: Mapper -> XML; up: XML -> Mapper', () => {
    const down = resolveDir(mapper, 'selectById(Long id)', 'down');
    const up = resolveDir(xml, '<select id="selectById"', 'up');
    if (!down.some((l) => l.uri.includes(xml))) throw new Error('down: no xml');
    if (!up.some((l) => l.uri.includes(mapper))) throw new Error('up: no mapper');
    return 'ok';
  });

  check('D-05 fallback: XML has no down -> falls back to up (Mapper)', () => {
    const out = resolveDir(xml, '<select id="selectById"', 'down');
    if (!out.some((l) => l.uri.includes(mapper))) throw new Error('no mapper fallback: ' + JSON.stringify(out));
    return 'ok';
  });

  check('D-06 fallback: Controller has no up -> falls back to down (Service)', () => {
    const out = resolveDir(ctrl, 'getInfo(Long id)', 'up');
    if (!out.some((l) => l.uri.includes(svc))) throw new Error('no service fallback: ' + JSON.stringify(out));
    return 'ok';
  });

  check('D-07 entity: getter down -> field; field up -> getter', () => {
    const f = pos(entity, 'private Long id;', 'private Long '.length);
    const g = pos(entity, 'getId()');
    const down = resolveDir(entity, 'getId()', 'down');
    const up = resolveDir(entity, 'private Long id;', 'up', 'private Long '.length);
    if (!hasLine(down, entity, f.line)) throw new Error('getter down did not reach field line ' + f.line);
    if (!hasLine(up, entity, g.line)) throw new Error('field up did not reach getter line ' + g.line);
    return 'ok';
  });

  check('D-08 path hits carry file/verb/module', () => {
    const hits = addon.searchPathHits('/v1/user/getInfo');
    if (hits.length === 0) throw new Error('no hits');
    const h = hits[0];
    if (!h.path.includes('/v1/user/getInfo')) throw new Error('bad path ' + h.path);
    if (!h.file.includes('UserController.java')) throw new Error('bad file ' + h.file);
    if (h.method !== 'GET') throw new Error('bad method ' + h.method);
    return `${h.path} ${h.file} [${h.method}] ${h.module}`;
  });

  check('D-09 multi-token path hits', () => {
    const hits = addon.searchPathHits('user getInfo');
    if (!hits.some((h) => h.path.includes('/v1/user/getInfo'))) {
      throw new Error('no multi-token hit: ' + JSON.stringify(hits.slice(0, 3)));
    }
    return `-> ${hits.length}`;
  });

  check('D-10 full path (class prefix + method), no bare /getInfo', () => {
    const hits = addon.searchPathHits('getInfo');
    if (!hits.some((h) => h.path === '/v1/user/getInfo')) {
      throw new Error('missing full /v1/user/getInfo');
    }
    if (hits.some((h) => h.file === 'UserController.java' && h.path === '/getInfo')) {
      throw new Error('bare /getInfo still indexed');
    }
    return `-> ${hits.length}`;
  });

  check('D-12 entity getter down -> exactly the field', () => {
    const f = pos(entity, 'private Long id;', 'private Long '.length);
    const out = resolveDir(entity, 'getId()', 'down');
    if (out.length !== 1) throw new Error('expected exactly 1, got ' + out.length);
    if (!hasLine(out, entity, f.line)) throw new Error('not the field');
    return 'ok';
  });

  check('L-01 Lombok @Data: getName() -> field', () => {
    const f = pos(dto, 'private String name;');
    const out = resolveDir(dtoCtrl, 'getName()', 'down');
    if (!hasLine(out, dto, f.line)) throw new Error('not field line ' + f.line);
    return 'ok';
  });

  check('L-02 Lombok @Data: setName() -> field', () => {
    const f = pos(dto, 'private String name;');
    const out = resolveDir(dtoCtrl, 'setName("n")', 'down');
    if (!hasLine(out, dto, f.line)) throw new Error('not field line');
    return 'ok';
  });

  check('L-03 Lombok @Data: isEnabled() -> boolean field', () => {
    const f = pos(dto, 'private boolean enabled;');
    const out = resolveDir(dtoCtrl, 'isEnabled()', 'down');
    if (!hasLine(out, dto, f.line)) throw new Error('not boolean field line');
    return 'ok';
  });

  check('L-04 explicit method wins over Lombok synthetic', () => {
    const m = pos(dto, 'public Long getId()');
    const f = pos(dto, 'private Long id;');
    const out = resolveDir(dtoCtrl, 'getId()', 'down');
    if (!hasLine(out, dto, m.line)) throw new Error('not explicit method line');
    if (hasLine(out, dto, f.line)) throw new Error('unexpectedly resolved to field');
    return 'ok';
  });

  check('L-05 static/final fields get no synthesized accessor', () => {
    const a = resolveDir(dtoCtrl, 'getSerialVersionUID()', 'down');
    const b = resolveDir(dtoCtrl, 'setCode("c")', 'down');
    if (a.length !== 0) throw new Error('static field should have no getter');
    if (b.length !== 0) throw new Error('final field should have no setter');
    return 'ok';
  });

  check('D-11 fuzzy tokens incl. slashes; empty -> none', () => {
    if (!addon.searchPathHits('/v1/user/getInfo').some((h) => h.path === '/v1/user/getInfo')) {
      throw new Error('slash fuzzy failed');
    }
    if (!addon.searchPathHits('/user list').some((h) => h.path === '/v1/user/getInfo')) {
      throw new Error('token fuzzy failed');
    }
    if (addon.searchPathHits('').length !== 0) {
      throw new Error('empty should be none');
    }
    return 'ok';
  });

  check('S-01 searchByPath /v1/user/getInfo -> Controller', () => {
    const out = addon.searchByPath('/v1/user/getInfo');
    if (!out.some((l) => l.uri.includes(ctrl))) throw new Error('no controller location: ' + JSON.stringify(out));
    return `-> ${out.length} location(s)`;
  });

  check('M-02 results deduped & sorted', () => {
    const { out } = resolve(svc, 'getInfo(Long id)');
    const keys = out.map((l) => `${l.uri}:${l.line}:${l.character}`);
    if (new Set(keys).size !== keys.length) throw new Error('duplicates present');
    for (let i = 1; i < keys.length; i++) {
      if (keys[i - 1] > keys[i]) throw new Error('not sorted');
    }
    return 'ok';
  });

  check('C-04 reloadConfig is callable', () => {
    addon.reloadConfig();
    return 'ok';
  });

  check('F-03 updateFile is callable', () => {
    addon.updateFile(addon.uriForPath(path.join(root, ctrl)));
    return 'ok';
  });

  async function checkAsync(name, fn) {
    try {
      const info = await fn();
      console.log(`  PASS  ${name}${info ? '  ' + info : ''}`);
      results.push(true);
    } catch (e) {
      console.log(`  FAIL  ${name}: ${e.message}`);
      results.push(false);
    }
  }

  await checkAsync('UP-01 updateFiles is idempotent (no growth)', async () => {
    const before = addon.getStatus();
    const uri = addon.uriForPath(path.join(root, ctrl));
    addon.updateFiles([uri]);
    const r = addon.updateFiles([uri]);
    if (r.files !== before.files) throw new Error(`files ${before.files} -> ${r.files}`);
    if (r.methods !== before.methods) throw new Error(`methods ${before.methods} -> ${r.methods}`);
    return `files=${r.files} methods=${r.methods}`;
  });

  await checkAsync('UP-02 updateFiles picks up a changed file', async () => {
    const file = path.join(root, ctrl);
    const before = addon.getStatus();
    const orig = fs.readFileSync(file, 'utf8');
    fs.writeFileSync(
      file,
      orig.replace('public void ping() {}', 'public void ping() {}\n\n    public void addedByTest() {}')
    );
    const r = addon.updateFiles([addon.uriForPath(file)]);
    fs.writeFileSync(file, orig);
    addon.updateFiles([addon.uriForPath(file)]);
    if (r.methods !== before.methods + 1) throw new Error(`methods ${before.methods} -> ${r.methods}`);
    return `methods ${before.methods} -> ${r.methods}`;
  });

  // ---- multi-root / multi-module ----
  const parent = fs.mkdtempSync(path.join(os.tmpdir(), 'springnav-multi-'));
  function wm(rel, content) {
    const p = path.join(parent, rel);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, content);
  }
  wm('proj-a/pom.xml', '<project/>');
  wm('proj-a/mod-core/pom.xml', '<project/>');
  wm(
    'proj-a/mod-core/src/main/java/a/core/CoreService.java',
    'package a.core;\npublic class CoreService {\n    public void core() {}\n}\n'
  );
  wm('proj-a/mod-web/pom.xml', '<project/>');
  wm(
    'proj-a/mod-web/src/main/java/a/web/CoreController.java',
    'package a.web;\nimport org.springframework.web.bind.annotation.*;\n@RestController\n@RequestMapping("/core")\npublic class CoreController {\n    @GetMapping("/list")\n    public void list() {}\n}\n'
  );
  wm('proj-b/pom.xml', '<project/>');
  wm(
    'proj-b/src/main/java/b/BController.java',
    'package b;\nimport org.springframework.web.bind.annotation.*;\n@RestController\n@RequestMapping("/b")\npublic class BController {\n    @GetMapping("/x")\n    public void x() {}\n}\n'
  );

  await checkAsync('MR-01 multi-root: both projects indexed, modules correct', async () => {
    const r = await addon.buildIndex([path.join(parent, 'proj-a'), path.join(parent, 'proj-b')]);
    if (r.projects !== 2) throw new Error('projects=' + r.projects);
    if (r.modules !== 3) throw new Error('modules=' + r.modules);
    const core = addon.searchPathHits('/core/list');
    if (!core.some((h) => h.module === 'mod-web')) {
      throw new Error('mod-web missing: ' + JSON.stringify(core.map((h) => h.module)));
    }
    const b = addon.searchPathHits('/b/x');
    if (!b.some((h) => h.module === 'proj-b')) {
      throw new Error('proj-b missing: ' + JSON.stringify(b.map((h) => h.module)));
    }
    return `projects=${r.projects} modules=${r.modules} files=${r.files}`;
  });

  await checkAsync('MR-02 nested roots deduped (no double index)', async () => {
    const one = await addon.buildIndex([parent]);
    const two = await addon.buildIndex([parent, path.join(parent, 'proj-a')]);
    if (one.files !== two.files) throw new Error(`files ${one.files} vs ${two.files}`);
    if (two.projects !== 1) throw new Error('projects=' + two.projects);
    return `files=${two.files} projects=${two.projects}`;
  });

  await checkAsync('RS-01 reindexRoot rebuilds one project only', async () => {
    await addon.buildIndex([parent]);
    const before = addon.getStatus();
    wm(
      'proj-a/mod-web/src/main/java/a/web/NewController.java',
      'package a.web;\nimport org.springframework.web.bind.annotation.*;\n@RestController\n@RequestMapping("/new")\npublic class NewController {\n    @GetMapping("/y")\n    public void y() {}\n}\n'
    );
    const r = addon.reindexRoot(path.join(parent, 'proj-a'));
    if (r.files !== before.files + 1) throw new Error(`files ${before.files} -> ${r.files}`);
    if (!addon.searchPathHits('/new/y').length) throw new Error('new controller not indexed');
    if (!addon.searchPathHits('/b/x').length) throw new Error('proj-b lost after reindex');
    return `files ${before.files} -> ${r.files}`;
  });

  const failed = results.filter((r) => !r).length;
  console.log(`\n${results.length - failed}/${results.length} checks passed`);
  fs.rmSync(root, { recursive: true, force: true });
  fs.rmSync(parent, { recursive: true, force: true });
  process.exit(failed ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});
