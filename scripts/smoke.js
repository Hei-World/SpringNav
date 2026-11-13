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
  const count = await addon.buildIndex(root);
  console.log('indexed files:', count);
  console.log('stats:', addon.indexStats());

  const base = 'src/main/java/com/example/';
  const svc = base + 'service/UserService.java';
  const impl = base + 'service/impl/UserServiceImpl.java';
  const mapper = base + 'mapper/UserMapper.java';
  const baseMapper = base + 'mapper/BaseMapper.java';
  const xml = 'src/main/resources/mapper/UserMapper.xml';
  const ctrl = base + 'controller/UserController.java';
  const entity = base + 'domain/User.java';

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

  const failed = results.filter((r) => !r).length;
  console.log(`\n${results.length - failed}/${results.length} checks passed`);
  fs.rmSync(root, { recursive: true, force: true });
  process.exit(failed ? 1 : 0);
})().catch((e) => {
  console.error('FATAL', e);
  process.exit(1);
});
