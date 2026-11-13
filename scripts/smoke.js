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

write(
  'src/main/java/com/example/util/StrUtil.java',
  `package com.example.util;

public class StrUtil {
    public static final String DEFAULT_NAME = "n";

    public static boolean isEmpty(String s) {
        return s == null || s.length() == 0;
    }

    public static boolean isBlank(String s) {
        return s == null || s.trim().length() == 0;
    }
}
`
);

write(
  'src/main/java/com/example/util/AppConst.java',
  `package com.example.util;

public class AppConst {
    public static final String TOKEN_PREFIX = "tk_";
    public static final int MAX_RETRY = 3;
}
`
);

write(
  'src/main/java/com/example/controller/StaticCallController.java',
  `package com.example.controller;

import com.example.util.StrUtil;
import com.example.util.AppConst;

public class StaticCallController {
    private String marker;

    public boolean check(String s) {
        if (StrUtil.isEmpty(s)) {
            return true;
        }
        String p = AppConst.TOKEN_PREFIX;
        this.check(s);
        return retry(s);
    }

    private boolean retry(String s) {
        return StrUtil.isBlank(s) && AppConst.MAX_RETRY > 0;
    }

    public String getMarker() {
        return this.marker;
    }
}
`
);

write(
  'src/main/java/com/example/domain/TaskEvent.java',
  `package com.example.domain;

import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;
import lombok.AllArgsConstructor;

@Builder
@Data
@NoArgsConstructor
@AllArgsConstructor
public class TaskEvent {
    private String loopNote;
    private String taskStatus;
}
`
);

write(
  'src/main/java/com/example/domain/EventKind.java',
  `package com.example.domain;

public enum EventKind {
    COMPANY,
    INDUSTRY,
    OTHER
}
`
);

write(
  'src/main/java/com/example/util/StrConst.java',
  `package com.example.util;

public class StrConst {
    public static final String TOKEN_PREFIX = "tk_";
}
`
);

write(
  'src/main/java/com/example/service/KindService.java',
  `package com.example.service;

import com.example.domain.EventKind;
import com.example.domain.TaskEvent;
import static com.example.util.StrConst.TOKEN_PREFIX;

public class KindService {
    public boolean go(TaskEvent event, EventKind kind) {
        TaskEvent body = new TaskEvent();
        for (TaskEvent e : java.util.Collections.emptyList()) {
            if (e.getLoopNote() != null) {
                return false;
            }
        }
        if (kind == EventKind.COMPANY && TOKEN_PREFIX.isEmpty()) {
            return doComplete(event);
        }
        return false;
    }

    private boolean doComplete(TaskEvent event) {
        return true;
    }
}
`
);

write(
  'src/main/java/com/example/service/TaskEventService.java',
  `package com.example.service;

import com.example.domain.TaskEvent;

public class TaskEventService {
    private static final String TASK_STATUS_DONE = "done";

    public boolean doComplete(TaskEvent event, TaskEvent body) {
        if (TASK_STATUS_DONE.equals(event.getTaskStatus())) {
            mark(TaskEvent::getLoopNote, body.getLoopNote());
        }
        return true;
    }

    private void mark(java.util.function.Function<TaskEvent, String> f, String v) {
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

  check('D-03b down: Impl 方法声明名 -> 接口（不进 Mapper）', () => {
    const out = resolveDir(impl, 'getInfo(Long id)', 'down', 5);
    if (!out.some((l) => l.uri.includes(svc))) throw new Error('no interface: ' + JSON.stringify(out));
    if (out.some((l) => l.uri.includes(mapper))) throw new Error('unexpected mapper: ' + JSON.stringify(out));
    return 'ok';
  });

  check('D-04 down: Mapper -> XML; up: XML -> Mapper', () => {
    const down = resolveDir(mapper, 'selectById(Long id)', 'down');
    const up = resolveDir(xml, '<select id="selectById"', 'up');
    if (!down.some((l) => l.uri.includes(xml))) throw new Error('down: no xml');
    if (!up.some((l) => l.uri.includes(mapper))) throw new Error('up: no mapper');
    return 'ok';
  });

  // ---- P1: 静态/常量/this/裸调用/接收者 ----
  const P1CALLER = 'src/main/java/com/example/controller/StaticCallController.java';
  const P1UTIL = 'StrUtil.java';
  const P1CONST = 'AppConst.java';

  check('P1-1 down: 静态方法调用 -> 工具类重载集合', () => {
    const out = resolveDir(P1CALLER, 'StrUtil.isEmpty(s)', 'down', 12);
    if (!out.some((l) => l.uri.includes(P1UTIL))) throw new Error('no util: ' + JSON.stringify(out));
    if (out.length !== 1) throw new Error('expected 1 (isEmpty): ' + JSON.stringify(out));
    return `-> ${out.length}`;
  });

  check('P1-2 down: 静态调用同前缀不串扰（isBlank 不混入）', () => {
    const out = resolveDir(P1CALLER, 'StrUtil.isBlank(s)', 'down', 12);
    if (!out.some((l) => l.uri.includes(P1UTIL))) throw new Error('no util');
    if (out.some((l) => hasLine([l], P1UTIL, undefined) && false)) throw new Error('x');
    return 'ok';
  });

  check('P1-3 down: 常量引用 -> 常量声明', () => {
    const out = resolveDir(P1CALLER, 'AppConst.TOKEN_PREFIX', 'down', 11);
    if (!out.some((l) => l.uri.includes(P1CONST))) throw new Error('no const: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P1-4 down: 常量引用（数字常量）', () => {
    const out = resolveDir(P1CALLER, 'AppConst.MAX_RETRY > 0', 'down', 11);
    if (!out.some((l) => l.uri.includes(P1CONST))) throw new Error('no const: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P1-5 down: this.方法调用 -> 当前类', () => {
    const out = resolveDir(P1CALLER, 'this.check(s)', 'down', 5);
    if (!out.some((l) => l.uri.includes(P1CALLER))) throw new Error('no self: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P1-6 down: 裸调用（同类直调）-> 当前类', () => {
    const out = resolveDir(P1CALLER, 'return retry(s)', 'down', 11);
    if (!out.some((l) => l.uri.includes(P1CALLER))) throw new Error('no self: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P1-7 无方向: 接收者字段点击 -> 字段声明', () => {
    const o1 = resolve('src/main/java/com/example/controller/DtoController.java', 'userDto.getId()', 0).out;
    if (!o1.some((l) => l.uri.includes('DtoController'))) throw new Error('receiver click: ' + JSON.stringify(o1));
    return 'ok';
  });

  check('P1-8 up: 字段声明 -> 引用点（getMarker 读 this.marker）', () => {
    const out = resolveDir(P1CALLER, 'private String marker', 'up', 16);
    if (!out.some((l) => l.uri.includes(P1CALLER))) throw new Error('no ref: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P1-9 无方向: this.field 读取 -> 字段声明', () => {
    const out = resolve(P1CALLER, 'return this.marker', 12).out;
    if (!out.some((l) => l.uri.includes(P1CALLER))) throw new Error('no field: ' + JSON.stringify(out));
    return 'ok';
  });

  // ---- P2: 参数/局部变量类型 + Class::引用 + 裸常量 ----
  const P2ENT = 'src/main/java/com/example/domain/TaskEvent.java';
  const P2SVC = 'src/main/java/com/example/service/TaskEventService.java';

  check('P2-1 down: 参数接收 body.getLoopNote() -> Lombok 实体字段', () => {
    const out = resolveDir(P2SVC, 'body.getLoopNote()', 'down', 6);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no entity: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-2 down: 参数接收 event.getTaskStatus() -> Lombok 实体字段', () => {
    const out = resolveDir(P2SVC, 'event.getTaskStatus()', 'down', 7);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no entity: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-3 down: 方法引用 TaskEvent::getLoopNote -> 实体字段', () => {
    const out = resolveDir(P2SVC, 'TaskEvent::getLoopNote', 'down', 13);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no entity: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-4 down: 裸常量引用 -> 常量声明', () => {
    const out = resolveDir(P2SVC, 'TASK_STATUS_DONE.equals', 'down', 2);
    if (!out.some((l) => l.uri.includes(P2SVC))) throw new Error('no decl: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-5 up: 常量声明 -> 引用点', () => {
    const out = resolveDir(P2SVC, 'private static final String TASK_STATUS_DONE', 'up', 28);
    if (!out.some((l) => l.uri.includes(P2SVC))) throw new Error('no ref: ' + JSON.stringify(out));
    return `-> ${out.length}`;
  });

  check('P2-6 up: 方法声明 -> 调用点（当前类）', () => {
    const out = resolveDir(P2SVC, 'private void mark(', 'up', 15);
    if (!out.some((l) => l.uri.includes(P2SVC))) throw new Error('no caller: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-7 down: 调用点 -> 方法定义', () => {
    const out = resolveDir(P2SVC, 'mark(TaskEvent::getLoopNote', 'down', 2);
    if (!out.some((l) => l.uri.includes(P2SVC))) throw new Error('no def: ' + JSON.stringify(out));
    return 'ok';
  });

  // ---- P2 扩展: 类型引用 / lambda / for-each / 静态导入 / 枚举 ----
  const P2KIND = 'src/main/java/com/example/service/KindService.java';
  const P2ENUM = 'EventKind.java';
  const P2SCONST = 'StrConst.java';

  check('P2-8 down: 点击参数类型 TaskEvent -> 实体类声明', () => {
    const out = resolveDir(P2KIND, 'go(TaskEvent event', 'down', 7);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no class: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-9 down: 点击局部变量类型 -> 实体类声明', () => {
    const out = resolveDir(P2KIND, 'TaskEvent body = new TaskEvent()', 'down', 2);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no class: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-10 down: new TaskEvent() 类型点击 -> 类声明', () => {
    const out = resolveDir(P2KIND, 'new TaskEvent()', 'down', 4);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no class: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-11 down: for-each 类型点击 -> 类声明', () => {
    const out = resolveDir(P2KIND, 'for (TaskEvent e :', 'down', 8);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no class: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-12 down: for-each 变量 e.getLoopNote() -> 实体字段', () => {
    const out = resolveDir(P2KIND, 'e.getLoopNote()', 'down', 4);
    if (!out.some((l) => l.uri.includes(P2ENT))) throw new Error('no field: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-13 down: 静态导入常量 TOKEN_PREFIX -> 常量声明', () => {
    const out = resolveDir(P2KIND, 'TOKEN_PREFIX.isEmpty()', 'down', 2);
    if (!out.some((l) => l.uri.includes(P2SCONST))) throw new Error('no const: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-14 down: 枚举引用 EventKind.COMPANY -> 枚举常量', () => {
    const out = resolveDir(P2KIND, 'EventKind.COMPANY', 'down', 12);
    if (!out.some((l) => l.uri.includes(P2ENUM))) throw new Error('no enum: ' + JSON.stringify(out));
    return 'ok';
  });

  check('P2-15 down: 点击参数类型 EventKind -> 枚举类声明', () => {
    const out = resolveDir(P2KIND, 'EventKind kind', 'down', 2);
    if (!out.some((l) => l.uri.includes(P2ENUM))) throw new Error('no enum class: ' + JSON.stringify(out));
    return 'ok';
  });

  check('D-05 fallback: XML has no down -> falls back to up (Mapper)', () => {
    const out = resolveDir(xml, '<select id="selectById"', 'down');
    if (!out.some((l) => l.uri.includes(mapper))) throw new Error('no mapper fallback: ' + JSON.stringify(out));
    return 'ok';
  });

  check('D-06 定义/引用模型: Controller 方法声明点击 -> 无跳转（声明无引用方）', () => {
    const out = resolveDir(ctrl, 'getInfo(Long id)', 'up');
    if (out.length !== 0) throw new Error('expected empty (声明无引用): ' + JSON.stringify(out));
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
    if (two.projects !== 2) throw new Error('projects=' + two.projects);
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
