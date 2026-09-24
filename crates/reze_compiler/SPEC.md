# reze_compiler — спецификация

Статус: целевое поведение для переписывания. Компилятор обязан соответствовать этому документу.
Всё, чего здесь нет, — вне рамок, а не на усмотрение реализации.

Принцип: **семантику задаёт рантайм, компилятор только убирает лишнее.** Любая оптимизация
(§8) обязана сохранять наблюдаемое поведение: `optimize: false` и `optimize: true` ведут себя одинаково.

## 1. Задачи и не-задачи

**Задачи:**

1. JSX → прямые DOM-операции (client), захват существующего DOM (hydrate), конкатенация строк (server).
2. Анализ всей программы: константные сигналы, клиентские и статические компоненты, границы островов,
   набор фич рантайма на каждый остров.
3. Отдавать результаты анализа в LSP в виде диагностики, подсказок и объяснений.

**Не-задачи:**

- Проверка типов (это делает TypeScript).
- Бандлинг, минификация, разрешение модулей (это делают Rolldown/Vite; компилятор получает от них граф).
- Трансформация не-JSX кода моделей данных, кроме удаления константных сигналов, инлайна `computed`
  (§8), снятия Proxy со store (§15.6) и переписывания деструктуризации props (§15.7).

## 2. Этапы

| Этап   | Содержание                                                                                                                                        | Статус |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| M1     | Новая архитектура (§4), target `Client`, система диагностики (§9), помодульные оптимизации O1–O3, O5 (§8), исправление багов v0 (§11)             | готово |
| M2     | Targets `Server` и `Hydrate` на том же IR (§14)                                                                                                   | готово |
| **M3** | Анализ программы: `ModuleSummary` → `Facts`, острова, фичи на остров, межмодульная свёртка сигналов, снятие Proxy со store, define-флаги рантайма | §15    |
| **M4** | dev-анализ и HMR, ленивые острова, слоты, межмодульный O4, массивы в store, не-литеральные default, стриминг                                      | готово |
| M99    | `reze_lsp`: диагностика, inlay hints, «почему остров?» по цепочкам `Reason`                                                                       | §17    |

Требования M1 ради M2–M4 и M99: IR не содержит ничего специфичного для client (выбор target делается только в
`emit`). У каждого решения оптимизатора есть причина `Reason`, которую можно показать как `info`-диагностику
(§9.5). Диагностики сериализуются без потерь (§9.2), и LSP сможет отдать их как есть.

## 3. Контракт

```rust
pub fn compile(source: &str, filename: &str, options: &Options)
    -> Result<Option<Output>, Vec<Diagnostic>>;

pub struct Options {
    pub module_name: String, // "reze-js": откуда импортируется рантайм
    pub source_map: bool,    // true
    pub optimize: bool,      // true: O3, O5 (§8); O1, O2 работают всегда
    pub target: Target,      // Client | Server | Hydrate (§14); Client
}
pub struct Output { pub code: String, pub map: Option<String>, pub diagnostics: Vec<Diagnostic> }
```

- `Ok(None)`: в файле нечего переписывать (нет JSX и ни одной свёртки: O3, O4, store, props, решений из
  `facts`). Вызывающая сторона оставляет исходник как есть.
- `Err`: ошибки парсинга или любая диагностика с `severity: error`. Ничего не генерируется.
- `Ok(Some)`: `diagnostics` содержит `warn` и `info`.
- Диалект (`.tsx`, `.jsx`, `.ts`, `.js`) определяется по `filename`. Неизвестное расширение парсится как TSX.
  Синтаксис TypeScript (`as any`, аннотации) генерируется только для TS-диалектов.
- Каждый байт исходника вне заменяемых участков (§4, «дыры») копируется без изменений, TypeScript тоже.
- `reze_napi`: `compile(source, filename, { moduleName, sourceMap, optimize, target }) → { code?, map?, diagnostics } | null`,
  `target: "client" | "server" | "hydrate"`.
  Ошибки компиляции не бросаются: при `Err` возвращается `{ diagnostics }` без `code` и `map`, где есть
  хотя бы одна `error`. Бросается только сбой самого вызова (неверные аргументы).

## 4. Пайплайн и модули (M1)

```
parse (oxc_parser) ─▶ semantic (oxc_semantic: scopes, symbols, references)
      ─▶ analyze (факты O3 + причины как info-диагностики)
      ─▶ lower (AST → IR: все решения)
      ─▶ emit (IR → Code для target: никаких решений) ─▶ source map
```

| Модуль                     | Отвечает за                                                                                                                                             |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lib.rs`                   | `compile`, `Options`, `Output`                                                                                                                          |
| `diagnostic/mod.rs`        | `Diagnostic`, `Severity`, `Fix`, `Label`, сбор, рендер в текст и JSON; `LineIndex`                                                                      |
| `diagnostic/catalog.rs`    | единый каталог кодов (§9.4): severity, заголовок, объяснение, исправление, пример «плохо/хорошо»                                                        |
| `code.rs`                  | `Code`: буфер вывода + метки `(generated, source)`; source map v3                                                                                       |
| `html.rs`                  | таблицы элементов/атрибутов/событий, экранирование HTML и JS, очистка JSX-текста, сущности                                                              |
| `analyze.rs`               | распознавание примитивов рантайма по символу (§8.0), O3, геттеры сигналов для `SIGNAL_NOT_CALLED`                                                       |
| `ir.rs`                    | IR (§6)                                                                                                                                                 |
| `lower/mod.rs`             | `Lowerer`, поиск дыр в JS-участке (`Embed`), путь компонента для диагностики                                                                            |
| `lower/element.rs`         | нативные элементы → `Template` (HTML, дерево узлов, обходы, операции)                                                                                   |
| `lower/attribute.rs`       | классификация атрибутов (§7.2), статическая свёртка                                                                                                     |
| `lower/component.rs`       | компоненты, props, дети компонентов                                                                                                                     |
| `lower/children.rs`        | списки детей: правила JSX-текста, O5, вставки, условия, массивы детей                                                                                   |
| `lower/constant.rs`        | статическое вычисление (`static_text`, истинность), `is_dynamic`                                                                                        |
| `lower/async_component.rs` | план async-компонента (§7.9)                                                                                                                            |
| `emit/mod.rs`              | `Emitter`, склейка дыр, `Namer`, импорты `Helper`, таблица шаблонов (фабрики или строки server), шапка и хвост модуля, выбор эмиттера шаблона по target |
| `emit/template.rs`         | client и hydrate: IIFE шаблона (клон или `claim`), обходы, операции, слитый `bind`                                                                      |
| `emit/server.rs`           | server: HTML шаблона, разрезанный по динамическим частям, и `ssr` (§14.1)                                                                               |
| `emit/component.rs`        | `createComponent`, объекты props, `mergeProps`, дети                                                                                                    |
| `emit/async_component.rs`  | синхронная форма async-компонентов                                                                                                                      |

Инварианты:

- `lower` не создаёт JS-текст. `emit` не смотрит в AST и ничего не решает.
- Каждое выражение исходника опускается один раз и генерируется один раз. Повторное использование
  значения идёт только через временную переменную (§7.6).
- IR живёт в арене `oxc_allocator`: `&'a str`, аренные `Vec`/`Box`, ничего с `Drop`.
- Код пишется в один буфер `Code`. Отдельно собирается только шапка модуля: она зависит от всего тела.
- `semantic` строится один раз на файл и используется `analyze`, `lower` (разрешение символов) и
  генератором имён (все имена исходника зарезервированы).

## 5. Раскладка вывода

```
<исходник до конца: hashbang, директивы, ведущие import-объявления>
import { <export> as <alias>, … } from "<module_name>";
const <tmpl> = /*#__PURE__*/ <factory>("<html>"), …;  // server: const <tmpl> = ["<html до части>", …], …;
<остаток исходника, в котором дыры заменены>
<alias delegateEvents>(["<event>", …]);        // только если есть делегированные события; сортировка
```

- Импорты рантайма перечисляются в порядке первого использования. Псевдоним — `_$<export>`, с числовым
  суффиксом, если имя уже занято. Все сгенерированные имена избегают всех идентификаторов исходника.
- Шаблоны дедуплицируются по `(html, namespace)` (server — по набору строк) и объявляются в порядке первого
  использования.
- Фабрики: `template(html)`; `templateSVG("<svg>" + html + "</svg>")`, если корень — чисто SVG-элемент
  (`html::is_svg_element`); `templateMathML(html)` для корня `<math>`. Корень `<svg>` использует `template`.

## 6. IR

```rust
struct Embed<'a> { span: Span, holes: Vec<'a, Hole<'a>> }       // участок исходника; дыры отсортированы, не пересекаются
struct Hole<'a>  { span: Span, kind: HoleKind<'a> }
enum HoleKind<'a> {
    Jsx(Jsx<'a>),
    AsyncComponent(Box<'a, AsyncComponent<'a>>),
    ConstSignalDecl { getter: Span, init: Embed<'a> },           // O3: `[x, setX] = signal(v)` → `x = v`
    ConstSignalRead { getter: Span },                            // O3: `x()` → `x`
}

enum Jsx<'a> {
    Template(Box<'a, Template<'a>>),
    Component(Box<'a, Component<'a>>),
    Fragment(Vec<'a, Child<'a>>),
}

struct Template<'a> {
    html: &'a str,
    namespace: Namespace,                    // Html | Svg | MathMl
    nodes: Vec<'a, TemplateNode<'a>>,        // по NodeId, корень = 0 (элемент): где узел лежит в `html`
    walks: Vec<'a, Walk>,                    // порядок объявления == preorder
    ops: Vec<'a, Op<'a>>,                    // выполняются один раз, в порядке документа (кроме `<select value>`)
    binds: Vec<'a, Bind<'a>>,                // сливаются в один `bind` (O2)
    memo_count: u32,
}
enum TemplateNode<'a> {                      // смещения в байтах `html`
    Element { tag: &'a str, start: u32, attributes_end: u32, content_end: u32 }, // перед `>`; перед `</tag>`
    Leaf { start: u32 },                     // текст или маркер `<!>`
}
struct Walk { node: NodeId, from: From, next_siblings: u32 }     // From::FirstChildOf(NodeId) | From::Node(NodeId)

enum Op<'a> {
    Set    { node: NodeId, target: BindTarget<'a>, value: Value<'a> },
    Event  { node: NodeId, event: &'a str, handler: Handler<'a> },
    Ref    { node: NodeId, target: RefTarget<'a> },
    Spread { node: NodeId, props: Props<'a>, is_svg: bool, has_children: bool },
    Memo   { id: MemoId, test: Embed<'a> },                        // поднятое условие (§7.5)
    Insert { parent: NodeId, value: Child<'a>, anchor: Anchor, inserts_after: u32 }, // Only | Before(NodeId) | End;
}   // inserts_after: сколько следующих вставок того же родителя делят этот якорь (§14.2)
struct Bind<'a> { node: NodeId, target: BindTarget<'a>, value: Value<'a> }
enum BindTarget<'a> {
    Attr(&'a str), AttrNs(&'static str, &'a str), Bool(&'a str), Class, Style,
    Prop { name: &'a str, html: PropHtml },  // как свойство выглядит в HTML сервера (§14.1)
}
enum PropHtml { None, Attr, Bool, Text, Html }  // `prop:x`, `<select value>` | `value` | `checked`, `selected` | `textContent`, `<textarea value>` | `innerHTML`
enum Value<'a> { True, Str(&'a str), Expr(Embed<'a>), Jsx(Jsx<'a>), ClassParts(Vec<'a, Value<'a>>) }
// ClassParts: несколько источников class, слитых в массив в порядке исходника (§7.3)
enum Handler<'a> {
    Delegated { handler: Embed<'a>, data: Option<Embed<'a>> },  // `el.$$click = h`
    DelegatedDynamic(Embed<'a>),                                // `addEventListener(el, "click", h, true)`
    Direct(Embed<'a>),                                          // `addEventListener(el, name, h)`
}

enum Child<'a> { Text(&'a str), Jsx(Jsx<'a>), Expr(ExprChild<'a>) }
enum ExprChild<'a> {
    Static(Embed<'a>),                       // нет реактивного чтения: передаётся как значение
    Getter(Getter<'a>),                      // Call(Span) для голого `f()` → `f`; Thunk { body, parenthesize } → `() => e`
    Memo(Getter<'a>),                        // `memo(getter)`: динамический элемент массива детей
    Conditional(Box<'a, Conditional<'a>>),   // §7.5: `() => _c$() ? a : b`, `_c$` объявлен `Op::Memo`
}
struct Conditional<'a> { memo: MemoId, consequent: Embed<'a>, alternate: Option<Embed<'a>> }

struct Component<'a> { callee: Embed<'a>, props: Props<'a> }
struct Props<'a> { parts: Vec<'a, PropsPart<'a>> }               // слева направо
enum PropsPart<'a> { Object(Vec<'a, Prop<'a>>), Spread { value: Embed<'a>, is_dynamic: bool } }
enum Prop<'a> {
    Value  { key: &'a str, value: PropValue<'a> },
    Getter { key: &'a str, value: PropValue<'a> },
    ForwardRef(AssignTarget<'a>),
}
enum PropValue<'a> { True, Str(&'a str), Expr(Embed<'a>), Jsx(Jsx<'a>), Children(Vec<'a, Child<'a>>) }

enum RefTarget<'a> {
    Callback(Embed<'a>),                                   // `ref={(el) => …}`
    Assign(AssignTarget<'a>),                              // вызвать, если функция, иначе присвоить
    Expr(Embed<'a>),                                       // всё остальное: вызвать, если это функция
}
enum AssignTarget<'a> {
    Identifier(Span),                                      // `ref={el}`
    Member { object: Embed<'a>, key: MemberKey<'a> },      // `ref={a.b}`, `ref={a[k]}`, `ref={a.#p}`
}
```

## 7. Семантика JSX

### 7.1 Теги

- Нативный элемент: первая буква строчная, или в имени есть `-`, или форма `ns:name`. `Foo`, `a.b`,
  `this` — компоненты.
- Void-элементы (`html::is_void`, включая `search`) не имеют детей и закрывающего тега.
- Наследование пространства имён: дети `<svg>` и SVG-корней — SVG. Дети `<foreignObject>` снова HTML.

### 7.2 Атрибуты нативных элементов (без spread)

Правила проверяются по порядку. Результат зависит от значения: _литерал_ (строка, число, `true`,
`false`, `null`, `undefined` или свёрнутая константа, §7.10), _статическое_ (не реактивное по §7.11)
или _динамическое_.

| Атрибут                                      | Литерал                                                                                       | Статическое                      | Динамическое              |
| -------------------------------------------- | --------------------------------------------------------------------------------------------- | -------------------------------- | ------------------------- |
| `ref`                                        | —                                                                                             | `Op::Ref`                        | `Op::Ref`                 |
| `children`                                   | используется как дети, если вложенных детей нет; иначе игнорируется + `CHILDREN_PROP_IGNORED` |                                  |                           |
| `on:<name>`                                  | строка → HTML-атрибут `on<name>`                                                              | `addEventListener(el, name, h)`  | то же                     |
| `on<Upper>…`                                 | строка → HTML-атрибут                                                                         | делегированное или прямое (§7.4) | то же                     |
| `on<lower>…` с не-строковым значением        | `EVENT_NAME_LOWERCASE`, компилируется как `on<Upper>`                                         |                                  |                           |
| `class`, `className`, `classList`            | §7.3                                                                                          |                                  |                           |
| `style`                                      | строка → атрибут; объект из одних литералов → свёрнутый `a:b;c:d`                             | `style(el, v)`                   | bind `style(el, v, prev)` |
| `prop:<n>`                                   | `el.n = v`                                                                                    | `el.n = v`                       | bind                      |
| `attr:<n>`                                   | атрибут                                                                                       | `setAttribute`                   | bind                      |
| `bool:<n>`                                   | есть, если истинно                                                                            | `setBoolAttribute`               | bind                      |
| `xlink:*`, `xml:*`                           | атрибут                                                                                       | `setAttributeNS`                 | bind                      |
| `value`, `checked`, `selected` (только HTML) | атрибут шаблона, **кроме** `value` на `<textarea>`/`<select>` — там `Set` свойства            | `el.n = v`                       | bind                      |
| `textContent`, `innerHTML` (только HTML)     | `el.n = v`                                                                                    | `el.n = v`                       | bind                      |
| `key`                                        | атрибут + `KEY_ON_ELEMENT`                                                                    |                                  |                           |
| остальное                                    | атрибут (`true` → `name="true"`, голый → `name`, `false`/`null`/`undefined` → опущен)         | `setAttribute`                   | bind                      |

- Литеральный `true` даёт `name` для bool-целей и inline-свойств.
- Повтор одного имени: побеждает последний, диагностика `DUPLICATE_ATTRIBUTE`.
- Имя на расстоянии редактирования ≤ 2 от `html::KNOWN_ATTRIBUTES` даёт `UNKNOWN_ATTRIBUTE` с исправлением.
  Имена с `-` или `:` не проверяются.
- Для `<select value>` операция `Set` выполняется после `Insert`-операций элемента: опции уже существуют.
- Сигнал, переданный без вызова в атрибут, свойство или style (`title={count}`, где `count` по символу —
  геттер `signal`/`computed`), даёт `SIGNAL_NOT_CALLED` с исправлением `count()`. Детям и
  обработчикам событий функция передаётся законно, там диагностики нет.

### 7.3 `class` (Solid v2)

Единственный источник классов — `class`. Значение имеет тип `ClassValue = string | number | boolean |
null | undefined | Record<string, unknown> | ClassValue[]`, массивы могут быть вложенными.

- `className` и `classList` — устаревшие псевдонимы. Каждый даёт `CLASS_ALIAS` (warn) с исправлением
  «переименовать в `class`» и компилируется как `class`.
- Несколько источников класса на одном элементе (`class` + `classList`, `className` + `class`) сливаются
  в один массив в порядке исходника: `class="btn" classList={{ on: on() }}` → `className(el, ["btn", { on: on() }])`.
  Это не `DUPLICATE_ATTRIBUTE`: автор явно хотел оба источника.
- Свёртка: если все источники — строки или объекты/массивы со статическими ключами и литеральными
  значениями, получается статический `class="…"`. Токены идут в порядке исходника, без повторов, ложные
  отбрасываются (`0` в массиве остаётся как `"0"`, `true` в массиве — как `"true"`, как у рантайма; `0` как
  значение объекта — ложь). Если истинных нет, атрибут опускается.
- Со spread: ключи `className`/`classList` в литеральных атрибутах переименовываются в `class` (с той же
  диагностикой). Внутри spread-объектов рантайм их не обрабатывает (§13).
- Рантайм: `className(el, value, prev?)` сравнивает токены с `prev`, а без `prev` — с токенами, применёнными
  в прошлый раз (`el.$$class`).

### 7.4 События

- `onFooBar` → `foobar` в нижнем регистре, с одним псевдонимом: `doubleclick` → `dblclick`. Рантайм
  `spread` применяет то же отображение.
- Делегируемые (`html::is_delegated_event`, обязано совпадать с `DelegatedEvents` в `@rezejs/dom`):
  `click input change submit keydown keyup pointerdown pointerup pointermove focusin focusout`.
- Делегированное событие с литералом функции → `el.$$click = h`. `[h, data]` → `el.$$click = h; el.$$clickData = data`.
  Любое другое выражение → `addEventListener(el, "click", h, true)`. Имя события попадает в список
  `delegateEvents` модуля.
- `on:name` и неделегируемые события → `addEventListener(el, name, h)`.

### 7.5 Дети нативных элементов

- Текст подчиняется правилу пробелов JSX Babel/TS (`html::clean_jsx_text`) после декодирования сущностей.
  Соседние тексты сливаются. Свёрнутые константы (включая O3) становятся текстом. Фрагменты разворачиваются.
- Динамический ребёнок → `insert(parent, value, anchor)`:
  - единственный ребёнок родителя: без якоря (рантайм использует `textContent`);
  - за ним следует статический узел: якорь — этот узел;
  - между двумя текстами: якорь — комментарий `<!>`, добавленный в шаблон;
  - в конце: якорь `null`.

  Вставки выполняются в порядке документа: вставка родителя, стоящая перед дочерним элементом, идёт до
  операций этого элемента. Server вычисляет части в порядке HTML, и порядок создания шаблонов совпадает
  (§14.3).

- Условие: `test ? a : b` или `test && a`, где `test` динамический, а хотя бы одна ветка содержит JSX или
  реактивное чтение. Проверка мемоизируется как `!!test`, чтобы ветки пересоздавались только при смене
  истинности. Memo — поднятая локальная переменная шаблона (`Op::Memo`):
  `var _c$ = memo(() => !!(test));`, затем `insert(el, () => _c$() ? a : b, …)` (`_c$() && a` без `else`).
  В массивах детей фрагментов и компонентов условие — обычный динамический элемент `memo(() => …)`.
- `f()` с голым идентификатором, без аргументов и без type arguments передаётся как `f`. Любое другое
  динамическое выражение — как `() => expr`.

### 7.6 Refs

- `Callback` → `use(fn, el)`.
- `Identifier x` → `typeof x === "function" ? use(x, el) : x = el`.
- `Member` → `var _o$ = <object>[, _k$ = <key>], _r$ = _o$.<key>; typeof _r$ === "function" ? use(_r$, el) : _o$.<key> = el`.
  Объект и ключ вычисляются **ровно один раз**.
- `Expr` → `var _r$ = e; typeof _r$ === "function" && use(_r$, el)`.
- На компонентах `ref` пробрасывается методом `ref(r$) { … }` в тех же формах, с `_r$(r$)` вместо `use`.

### 7.7 Spread на нативных элементах

Если у элемента есть spread, все атрибуты, кроме `ref` (и `children`, когда есть вложенные дети),
собираются в один `Props` в порядке исходника → `spread(el, props, isSvg, hasChildren)`. Дети всё равно компилируются.

### 7.8 Компоненты и фрагменты

- `createComponent(Comp, props)`. Props — объектный литерал, либо единственное статическое spread-значение,
  либо `mergeProps(…parts)`, где динамические spread передаются как `() => e`.
- Значения props: литералы и статические выражения остаются как есть. Динамические выражения и JSX
  становятся геттерами `get k() { return v; }`. Ключи — голые идентификаторы или строки в кавычках.
- Дети: нет → нет ключа `children`. Один текст → `children: "…"`. Одно статическое выражение, включая
  функцию (render props) → `children: e`. Один JSX или одно динамическое выражение → геттер. Больше одного →
  `get children() { return [ … ]; }`, где каждый динамический элемент — `memo(getter)`.
- `<For each={[…]}>` с инлайновым литералом массива → `INLINE_EACH`.
- Деструктуризация props в параметрах компонента (`function C({ a })`, где `C` возвращает JSX) переписывается
  в чтения `props.a` (§15.7). `PROPS_DESTRUCTURED` (warn) остаётся, только когда переписать нельзя: тогда
  чтение происходит один раз в теле и теряет реактивность.
- Фрагмент: ноль детей → `[]`, один → сам элемент, несколько → `[ … ]`.

### 7.9 Async-компоненты

`async function` или `async`-стрелка с блочным телом считается async-компонентом, если её тело где-либо
содержит JSX. Async-функции без JSX никогда не трогаются: хелпер вроде
`async function load() { const r = await fetch(u); return r.json(); }` сохраняет семантику `Promise`.
Async-компонент переписывается в синхронную форму, если:

- каждый `await` вне вложенных функций — это весь инициализатор верхнеуровневого
  `const|let|var x = await e;` с одним декларатором (между ними могут стоять другие инструкции);
- до последнего `await` нет `return`/`throw`;
- тело заканчивается (не считая пустых инструкций) на `return expr;`;
- ни одна локальная переменная, объявленная в сегменте до `await`, не используется после него.
  Проверяется по символам (`oxc_semantic`), а не по именам.

Форма вывода (на каждый `await` `i`: сигналы `_val{i}$`/`_settled{i}$`, один `effect`-шаг загрузки,
связанный по epoch, и финальный `memo`, который бросает ошибку или возвращает хвост) такая же, как в v0.
`as any` на начальных значениях сигналов генерируется только для TS-диалектов. Аннотация `Promise<T>`
разворачивается в `T`. Любая другая аннотация `Promise…` даёт `ASYNC_RETURN_TYPE`. Кандидат, не прошедший
план, даёт `ASYNC_COMPONENT_SHAPE` (с `data.reason`) и остаётся как написан. Async-компоненты без
верхнеуровневого `await` не диагностируются.

### 7.10 Свёртка констант

`static_text` покрывает строковые литералы, целые `|n| < 1e15`, шаблонные литералы без выражений, цепочки
`+`, где в каждом `+` хотя бы одна сторона — строка (`1 + 2` не сворачивается), и чтения сигналов,
свёрнутых O3 с литеральным инициализатором.

### 7.11 Тест реактивности

Выражение динамическое, если его вычисление может прочитать реактивное состояние: содержит вызов,
tagged template или доступ к члену вне вложенных функций. Вызов геттера, свёрнутого O3, реактивным не
считается. Для детей компонента выражение динамическое также, если содержит JSX. Голые идентификаторы и
литералы — статические.

## 8. Оптимизации

Каждая оптимизация: условие применимости проверяется по символам, при сомнении — не применять. Каждое
применение порождает `info`-диагностику с причиной (§9.5). У каждой есть differential-тест (§10).

### 8.0 Распознавание примитивов

Примитив распознаётся **по разрешению символа**, а не по имени: reference, чей symbol — import binding
`signal`/`computed` из одного из модулей рантайма: `options.module_name`, `reze-js`, `@rezejs/signals`,
`@rezejs/dom`. Переименование при импорте (`import { signal as s }`) поддерживается. Namespace-импорт
(`import * as R`) и реэкспорты через модули программы — §15.4.

### O1. Шаблоны и статическая свёртка (всегда)

Статические поддеревья → одна строка HTML на форму; литеральные атрибуты, `style`, `class` и тексты
сворачиваются в шаблон (§7.2, §7.3, §7.10). Фабрики шаблонов помечены `/*#__PURE__*/`.

### O2. Слияние привязок и минимальная поверхность рантайма (всегда)

- Все динамические атрибуты шаблона компилируются в **один** `bind` с поатрибутным сравнением
  `v !== p[i] && (…)`. Один атрибут → `bind` без массива.
- Импортируются только используемые хелперы. SVG/MathML-парсеры — отдельные фабрики. `delegateEvents`
  генерируется только при наличии делегированных событий.

### O3. Константные сигналы (`optimize`)

`const [get, set] = signal(init[, options])` сворачивается, если одновременно:

- `signal` — примитив по §8.0; у декларатора нет аннотации типа; паттерн — массив из одного или двух
  идентификаторов без rest, значений по умолчанию и пропусков;
- у сеттера нет ни одного reference (или его нет в паттерне);
- каждый reference геттера — это callee вызова без аргументов, не опциональный (`get()`), без type arguments;
- `options` отсутствует или является объектным литералом, значения которого — литералы или функции.
- ни геттер, ни сеттер не экспортируются (`export const [x] = …` и `export { x }`): модуль не видит
  импортёров. Экспортированные сигналы сворачивает только анализ программы (§15.5). Исправление M1:
  раньше `export const [title] = signal("x")` сворачивался в `export const title = "x"`, и `title()` в
  импортёре бросал `TypeError`.

Переписывание: декларатор → `get = init` (инициализатор вычисляется один раз, как раньше), каждое
`get()` → `get`. Если `init` — литерал по §7.10, чтения в JSX становятся статическим текстом шаблона.
Пример: `const [title] = signal("Reze"); <h1>{title()}</h1>` → `const title = "Reze";` и шаблон `<h1>Reze</h1>`.

### O4. Инлайн `computed` (`optimize`, M3)

`const d = computed(() => expr)` инлайнится в место чтения, если одновременно:

- `computed` — примитив по §8.0; декларатор единственный в своём объявлении; у него нет аннотации типа;
- геттер — стрелка без параметров с телом-выражением;
- у `d` ровно один reference, и это вызов `d()` внутри динамического выражения JSX (bind, insert, getter
  prop), в той же функции, что и объявление, не внутри вложенной функции;
- каждый идентификатор в `expr` разрешается в место чтения в тот же символ (нет затенения между
  объявлением и чтением).

Переписывание: объявление удаляется, `d()` → `(expr)`. Узел графа исчезает, привязка читает источники
напрямую; отличие в числе пересчётов DOM не видно, потому что привязка сама сравнивает значения.

### O5. Мёртвые ветки JSX (`optimize`)

`{false && <X/>}`, `{true ? <A/> : <B/>}` и `{null}`/`{undefined}`/`{false}`/`{true}` среди детей, где
условие — литерал: мёртвая ветка не генерируется (включая её шаблон и импорты).

### Анализ программы (M3)

Межмодульная свёртка сигналов, снятие Proxy со store, статические компоненты и острова, фичи рантайма
и define-флаги — §15.

## 9. Диагностика

Цель: уровень Solid 2.0 dev-диагностики (стабильные коды, сообщения с готовым исправлением, руководство
по починке в пакете, машиночитаемый канал для агентов), но на этапе компиляции, с точными спанами и
**машинно-применимыми исправлениями**.

### 9.1 Правила сообщения

Сообщение начинается с кода в скобках и состоит из трёх частей: что увидели → чем это плохо → что
сделать. Пример:
`[CLASS_ALIAS] \`classList\` is a legacy alias: Reze has one \`class\` attribute that takes strings, objects and arrays. Rename it to \`class\`; it was compiled as \`class\`.`
Сообщения на английском, одно-два предложения, с конкретными именами из кода, без «may»/«possibly».

### 9.2 Структура

```ts
interface Diagnostic {
  code: string; // стабильный SCREAMING_SNAKE, никогда не переиспользуется
  severity: "error" | "warn" | "info";
  message: string; // "[CODE] …" по §9.1
  file: string;
  start: { offset: number; line: number; column: number }; // line 1-based, column 0-based UTF-16
  end: { offset: number; line: number; column: number };
  path: string[]; // ["<App>", "<TodoList>", "li", "button"]: компоненты и элементы от корня функции
  labels: { start: number; end: number; message: string }[]; // вторичные спаны (например, первый дубликат)
  related: { file: string; start: number; end: number; message: string }[]; // спаны в других модулях (M3, §15.10): цепочка `Reason`
  fixes: { title: string; edits: { start: number; end: number; text: string }[] }[]; // применимы как есть
  data: Record<string, string>; // структурированные факты (имя атрибута, причина…)
  docs: string; // https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md#<code в нижнем регистре>
  rendered: string; // текстовый рендер §9.3 без подвала: единственный рендерер — в Rust
}
```

Rust: `Diagnostic { code: Code, severity, span, labels, fixes, data, path, related }`. `Code` — enum каталога.
Позиции вычисляются одним `LineIndex` в конце компиляции; у `related` — только смещения, строки и колонки
в чужих файлах компилятор не знает.

### 9.3 Текстовый рендер (консоль Vite, `Err` из napi)

```
[CLASS_ALIAS] `classList` is a legacy alias: … Rename it to `class`.
  in <Counter> › output
  at src/Counter.tsx:8:15
   7 |     <section class="counter">
>  8 |       <output classList={{ negative: count() < 0 }}>{count()}</output>
     |               ^^^^^^^^^
  fix: rename `classList` to `class`
```

Первое появление каждого кода за сборку заканчивается подвалом:

```
  repair guide: node_modules/@rezejs/compiler/skills/compiler-diagnostics/SKILL.md#class_alias
                https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md#class_alias
```

`info` в консоль не печатается.

### 9.4 Каталог и руководство по починке

- `diagnostic/catalog.rs` — единственный источник: для каждого кода severity, заголовок, что наблюдалось,
  почему это дефект, исправление, пример «плохо → хорошо».
- `packages/compiler/skills/compiler-diagnostics/SKILL.md` генерируется из каталога. Тест сверяет файл
  с каталогом; `REZE_UPDATE_SKILL=1 cargo test` перезаписывает его. Файл входит в `files` пакета.
- Правило для агента в шапке SKILL.md: не подавлять непонятную диагностику; применять `fixes`, если они
  есть; иначе чинить по разделу кода.

| Код                        | Severity | Триггер                                                                           | Исправление (`fixes`)         |
| -------------------------- | -------- | --------------------------------------------------------------------------------- | ----------------------------- |
| `PARSE_ERROR`              | error    | диагностика `oxc_parser`                                                          | —                             |
| `CLASS_ALIAS`              | warn     | `className`/`classList` на нативном элементе или в литеральном атрибуте со spread | переименовать в `class`       |
| `CHILDREN_PROP_IGNORED`    | warn     | атрибут `children` и вложенные дети одновременно                                  | удалить атрибут               |
| `KEY_ON_ELEMENT`           | warn     | `key` на нативном элементе                                                        | удалить атрибут               |
| `DUPLICATE_ATTRIBUTE`      | warn     | одно имя дважды на элементе                                                       | удалить ранний (label на нём) |
| `UNKNOWN_ATTRIBUTE`        | warn     | почти-совпадение с известным атрибутом                                            | переименовать в подсказку     |
| `EVENT_NAME_LOWERCASE`     | warn     | `onclick={fn}` — не-строковое значение у `on<lower>`                              | переименовать в `onClick`     |
| `SIGNAL_NOT_CALLED`        | warn     | геттер сигнала без вызова в атрибуте/свойстве/style                               | `count` → `count()`           |
| `PROPS_DESTRUCTURED`       | warn     | деструктуризация props, которую нельзя переписать (§15.7); `data.reason`          | — (описание в SKILL)          |
| `INLINE_EACH`              | warn     | `<For each={[…]}>`                                                                | —                             |
| `ASYNC_COMPONENT_SHAPE`    | warn     | async-компонент вне поддерживаемой формы; `data.reason`                           | —                             |
| `ASYNC_RETURN_TYPE`        | warn     | аннотация `Promise`, которую нельзя развернуть                                    | —                             |
| `SIGNAL_FOLDED`            | info     | применена O3; `data.signal`, `data.scope`: `module` \| `program` (§15.5)          | —                             |
| `DEAD_BRANCH_REMOVED`      | info     | применена O5                                                                      | —                             |
| `COMPUTED_INLINED`         | info     | применена O4; `data.computed`, `data.scope`: `module` \| `program` (§16.5)        | —                             |
| `PROPS_REWRITTEN`          | info     | деструктуризация props переписана (§15.7); `data.component`                       | —                             |
| `STORE_UNPROXIED`          | info     | store заменён сигналами полей (§15.6); `data.store`, `data.scope`                 | —                             |
| `STATIC_COMPONENT`         | info     | компонент статический (§15.8); `data.component`                                   | —                             |
| `CLIENT_COMPONENT`         | info     | компонент клиентский; `data.reason`, цепочка в `related`                          | —                             |
| `ISLAND`                   | info     | граница острова (§15.9); `data.id`, `data.features` (§15.11), `data.mode` (§16.3) | —                             |
| `LAZY_ISLAND`              | info     | остров с отложенной загрузкой (§16.3); `data.id`, `data.mode`                     | —                             |
| `ISLAND_DIRECTIVE_IGNORED` | warn     | `island:*` вне граничной позиции (§16.3);                                         | удалить атрибут               |
| `FACTS_STALE`              | error    | `facts` построены для другого исходника (§15.2)                                   | —                             |
| `PROGRAM_OPEN_IMPORT`      | error    | модуль вне программы импортирует закрытый модуль (§15.3)                          | —                             |
| `FEATURE_FLAG_MISMATCH`    | error    | модуль вне программы использует фичу, выключенную флагом (§15.11)                 | —                             |

### 9.5 Каналы

- `Output.diagnostics` / napi: полный JSON по §9.2, включая `info` (это объяснения оптимизатора для LSP в M99).
- Vite-плагин: `warn` → `this.warn` с рендером §9.3; `error` → ошибка с `loc`/`frame`/`id` для оверлея.
  Опция `diagnostics: { jsonl?: string }` дописывает каждую диагностику (все severity) строкой JSON в файл —
  канал для агентов и CI.

## 10. Source maps

Каждый скопированный участок исходника размечается в начале и в начале каждой строки внутри. Каждая
замена JSX размечается в начале, каждое встроенное выражение пользователя — как скопированный участок.
Колонки в UTF-16.

## 11. Баги v0, исправляемые в M1

Каждому — регрессионный snapshot:

1. `ref={refs[i++]}` вычислял выражение дважды (так же проброс `ref` компоненту).
2. `<textarea value>`/`<select value>` записывались атрибутом, который браузер игнорирует.
3. Два `class` на элементе: браузер молча брал первый.
4. `onDoubleClick` слушал несуществующее событие `doubleclick`.
5. Async-хелперы без JSX переписывались в реактивный код и переставали возвращать `Promise`.
6. `undefined as any` генерировался в `.jsx`.

## 12. Тесты

- `tests/snapshots.rs`: `insta`-снапшоты полного вывода, по одному на каждую возможность §5–§8, плюс §11;
  каждый вход — для всех трёх target (`server__…`, `hydrate__…`).
- `tests/compile.rs`: поведенческие утверждения. Вывод парсится как TSX/JSX без семантических ошибок;
  диагностики и их `fixes` по §9 (применение `fixes` даёт файл без этой диагностики); source maps;
  `Ok(None)` без JSX.
- Differential: каждый снапшот-вход компилируется с `optimize: true` и `false`; оба вывода валидны.
  В `packages/dom/tests` сценарии O3 и O5 прогоняются в happy-dom в обоих режимах, DOM после каждого шага совпадает.
- `tests/catalog.rs`: SKILL.md совпадает с каталогом; у каждого кода каталога есть тест, который его вызывает.
- `packages/dom/tests`: сквозное поведение рантайма через Vite-плагин. `hydrate.spec.ts`: сценарий
  компилируется для трёх target; `renderToString` даёт тот же видимый DOM, что client; `hydrate` оставляет
  каждый серверный узел на месте и не меняет разметку; после каждого шага и события гидратированный DOM
  совпадает с client.

## 13. Изменения рантайма и пакетов (M1)

- `@rezejs/dom`: удалить экспорт `classList` и обработку `classList`/`className` в `spread`; `className`
  отслеживает только `$$class`; в `spread` события `doubleclick` → `dblclick`.
- `@rezejs/vite-plugin`: рендер §9.3, подвал раз на код, опции `optimize` и `diagnostics.jsonl`.
- `@rezejs/compiler`: новый тип `Diagnostic` в `index.d.ts`, `skills/` в `files`.
- `reze_napi`: контракт §3.

## 14. Server и Hydrate (M2)

Три target получают один и тот же IR; различается только эмиттер шаблона. Компоненты, props, фрагменты,
дети компонентов, async-компоненты и O3 генерируются одинаково.

### 14.1 Server

- Шаблон → `ssr(<tmpl>, …parts)`: `<tmpl>` — строки `html`, разрезанного в точках частей по `TemplateNode`;
  `ssr` склеивает их в `RenderedHTML`, который `ssrChild` выводит без экранирования.
- Части стоят на своём месте в HTML (при равном месте — порядок ops, затем binds):
  - корень, `attributes_end`: `ssrHydrationKey()` → ` data-hk="…"` (§14.3);
  - `attributes_end`: `Attr`, `AttrNs`, `Prop { html: Attr }` → `ssrAttribute(name, v)`; `Bool`,
    `Prop { html: Bool }` → `ssrBoolAttribute(name, v)`; `Class` → `ssrClass(v)`; `Style` → `ssrStyle(v)`;
    `Spread` → `ssrSpread(props, isSvg)`;
  - `content_end`: `Prop { html: Text }` → `ssrChild(v)`; `Prop { html: Html }` → `ssrRaw(v)`; `Spread` без
    вложенных детей → `ssrChild(props.children)`, `props` тогда вычисляется один раз во временную `_s$`;
    `Insert` с якорем `Only`/`End`;
  - `start` якоря: `Insert` с якорем `Before`.
- `Insert` → `ssrChild(value)`, значение вычисляется на месте: `() => e` → `e`, `f` → `f()`, условие →
  `(test) ? a : b` (`: null` без `else`) без memo. Каждая вставка, кроме единственного ребёнка,
  обрамляется `<!--[-->…<!--]-->`.
- `Event`, `Ref` и `Prop { html: None }` ничего не выводят, их выражения не вычисляются; `delegateEvents` нет.
- Рантайм: `ssrAttribute` пропускает `null`/`undefined`/`false`; `ssrClass` пишет строку как есть, иначе
  истинные токены (как `className`); `ssrStyle` пишет строку или `key:value` без `null`; `ssrSpread` — то,
  что выставил бы `spread`, без событий, `ref`, `prop:`, `textContent`, `innerHTML` и `children`.
  `ssrChild` читает функции, разворачивает массивы, экранирует `&` и `<` в тексте, `null`/`boolean` → `""`.

### 14.2 Hydrate

- Как client, кроме: корень — `claim(tmpl, "<tag>")`; обходы — `claimChild(parent, k)` и
  `claimSibling(node, k)`, которые перешагивают диапазоны `<!--[-->…<!--]-->`; вставки —
  `claimInsert(parent, value[, anchor[, inserts_after]])`.
- `claimInsert` берёт уже отрисованное как `current` вставки: для единственного ребёнка — всё содержимое
  родителя; иначе — диапазон прямо перед якорем (или в конце родителя), пропустив `inserts_after`
  диапазонов следующих вставок. Закрывающий маркер становится якорем, маркеры остаются в DOM. Без
  диапазона (шаблон склонирован) это обычный `insert`.
- `hydrate(code, el)`: собирает `[data-hk]` внутри `el`, выполняет `code()` с ключами §14.3 и отдаёт
  корневой вставке содержимое `el`. После возврата `claim` снова клонирует. `spread` отдаёт вставке
  `children` уже отрисованное содержимое элемента.

### 14.3 Ключи гидратации

- Ключ шаблона — `scope.id + scope.count++`. `createComponent` открывает область
  `parent.id + parent.count++ + "-"`. Области активны во время `renderToString` и `hydrate`.
- Ключи совпадают, если каждый компонент создаёт шаблоны в одном порядке на сервере и на клиенте. Server
  вычисляет части в порядке HTML, client — ops в порядке документа (§7.5), поэтому шаблоны в детях
  создаются в одном порядке. Исключение — JSX внутри реактивного значения атрибута: client создаёт его в
  `bind` после всех ops, server — на месте атрибута. Такой JSX бессмыслен (атрибут получает строку
  объекта) и с гидратацией не поддерживается.

### 14.4 Границы M2

- `renderToString` синхронный: effects не выполняются, async-компоненты рендерят состояние до загрузки.
  Стриминг и Suspense на сервере — вне M2; `Suspense`, `Portal` и `Dynamic` со строковым тегом обращаются к
  `document` и на сервере не работают.
- `<select value>` сервер не отражает в HTML; клиент выставляет свойство при гидратации. `value`, `checked`,
  `selected` сервер пишет атрибутами.
- Соседние строки массива детей сервер склеивает в один текстовый узел; гидратация приводит DOM к виду
  client реконсиляцией (результат тот же, узлы пересоздаются).

### 14.5 Изменения рантайма и пакетов (M2)

- `@rezejs/dom`: `hydrate`, `claim`, `claimChild`, `claimSibling`, `claimInsert`, `renderToString`, `ssr`,
  `ssrChild`, `ssrHydrationKey`, `ssrAttribute`, `ssrBoolAttribute`, `ssrClass`, `ssrStyle`, `ssrSpread`,
  `ssrRaw`; области ключей в `createComponent`.
- `@rezejs/vite-plugin`: SSR-трансформы компилируются для `server`; опция `hydratable` переключает
  браузерные трансформы на `hydrate`.
- `reze_napi`: опция `target` (§3).

## 15. Анализ программы (M3)

Принцип §8 распространяется на программу: анализ программы только добавляет свёртки. `facts: None` и
`facts: Some` ведут себя одинаково, dev и build — тоже. Острова меняют только то, какой код выполняется
при гидратации: видимый DOM и поведение после гидратации не меняются (§15.9).

### 15.1 Режимы

| Режим       | Когда                                                                                     | Что работает                                                                                                                                                     |
| ----------- | ----------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Модульный   | `compile` без `facts`: `vite serve` (без `program.dev`, §16.2), тесты, сторонние сборщики | M2; O4 (§8); namespace-импорт рантайма (§15.4); переписывание props (§15.7); снятие Proxy с неэкспортированного store (§15.6)                                    |
| Программный | `compile` с `facts` из `link`: `vite build`, client и SSR сборки                          | всё модульное; реэкспорты примитивов; свёртка экспортированных сигналов (§15.5) и store (§15.6); статические компоненты и острова (§15.8, §15.9); флаги (§15.11) |

```
каждый файл программы: summarize(source) ─▶ ModuleSummary            (независимо, кешируемо, без AST)
плагин: разрешение спецификаторов (Vite) ─▶ link(modules) ─▶ ModuleFacts на модуль, фичи, closed
каждый модуль: compile(source, { facts }) ─▶ пайплайн §4; analyze берёт межмодульные решения из facts
```

### 15.2 Контракт

```rust
pub fn summarize(source: &str, filename: &str, options: &SummaryOptions)
    -> Result<ModuleSummary, Vec<Diagnostic>>;                 // SummaryOptions { module_name }
pub fn link(modules: &[ModuleInput], options: &LinkOptions) -> Linked;
pub fn verify(linked: &Linked, outside: &[OutsideModule]) -> Vec<Diagnostic>;

pub struct ModuleInput { pub id: String, pub summary: ModuleSummary, pub resolved: Vec<Option<String>>, pub is_entry: bool }
pub struct LinkOptions { pub optimize: bool, pub islands: bool, pub root: String } // root: `config.root` для id островов (§15.9)
pub struct Linked {
    pub facts: HashMap<String, ModuleFacts>,   // каждому модулю программы, даже без решений
    pub features: Features,                    // значения флагов (§15.11)
    pub closed: Vec<String>,                   // §15.3
}
pub struct OutsideModule { pub id: String, pub summary: Option<ModuleSummary>, pub imported: Vec<String> }
// `Options` (§3) получает `facts: Option<ModuleFacts>`.
```

- `summarize` принимает любой диалект, включая файлы без JSX: сигналы и store живут в `.ts`. Ошибки
  парсинга → `Err` с `PARSE_ERROR`. Сводка не зависит от target и `optimize`.
- `ModuleSummary.specifiers` — спецификаторы статических импортов, реэкспортов и литеральных `import()` в
  порядке первого появления; `resolved[i]` — id модуля программы для `specifiers[i]` или `None` (вне
  программы). Спецификаторы модулей рантайма (§8.0) распознаются по строке и не разрешаются.
- `ModuleSummary` и `ModuleFacts` — serde-структуры, через napi передаются JSON-строкой. Формат внутренний.
  В обоих есть `version` (версия крейта); чужая версия — сбой вызова (napi бросает), а не диагностика.
- `ModuleSummary.source_hash` — FNV-1a 64 байтов исходника, копируется в `ModuleFacts`. `compile` с фактами,
  чей хеш не совпадает с `source`, → `Err` с `FACTS_STALE`: другой pre-плагин изменил модуль между
  сканированием и `transform`.
- Решения о компонентах и островах не зависят от target и входов: SSR- и client-сборки одной программы
  приходят к одним и тем же решениям. Свёртки могут различаться, если различаются входы (§15.4), и каждая
  сохраняет поведение своей сборки.
- Инвариант: `link` одного модуля с `is_entry: true` даёт факты, с которыми вывод `compile` совпадает с
  выводом без фактов.

napi:

```ts
summarize(source, filename, { moduleName }) → { summary?: string; specifiers: string[]; diagnostics: Diagnostic[] }
link(modules: { id; summary; resolved: (string | null)[]; isEntry }[], { optimize, islands, root })
  → { facts: Record<string, string>; features: Record<string, boolean>; closed: string[] }
verify(linked, outside: { id; summary: string | null; imported: string[] }[]) → Diagnostic[]
compile(source, filename, { …, facts?: string })
```

### 15.3 Vite-плагин: программа и замкнутый мир

```ts
program?: boolean | { include?: RegExp; exclude?: RegExp }; // build: true; в serve игнорируется
islands?: boolean;                                           // требует hydratable; по умолчанию false
features?: Partial<Record<"hydration" | "suspense", true>>;  // принудительно включает флаг (§15.11)
```

- `buildStart`: программа — все файлы под `config.root` (рекурсивно, кроме `node_modules`, каталогов на `.`
  и `build.outDir`), чей путь проходит `include` (по умолчанию `/\.[cm]?[jt]sx?$/`) и не проходит `exclude`.
  Файл читается с диска и проходит `summarize`. Файл с ошибкой парсинга исключается из программы: если он
  попадёт в граф, его `transform` выдаст ту же ошибку. Спецификаторы разрешаются через `this.resolve(spec, id)`;
  результат вне программы → `null`. `is_entry` — модули, в которые разрешились `options.input` (HTML-входы
  Vite — не модули). Затем `link`.
- Программа — надмножество графа сборки: недостижимые файлы делают анализ только консервативнее.
- `transform` модуля программы — `compile` с его фактами; модуль вне программы компилируется без фактов.
- Замкнутый мир. Свёртка экспорта (§15.5, §15.6) переписывает и экспортирующий модуль, и его импортёров, а
  импортёр вне программы сломался бы. `closed` — модули программы, у которых хотя бы один экспорт участвует
  в таком решении. Модули пакетов рантайма (`reze-js`, `@rezejs/*`, пакет `moduleName`) не входят ни в
  программу, ни во «вне программы». Для остальных модулей вне программы, чей код упоминает спецификатор
  рантайма, плагин собирает `summary` из кода, пришедшего в `transform`. В `buildEnd` плагин по
  `this.getModuleIds()`/`getModuleInfo` (`importedIds` ∪ `dynamicallyImportedIds`) вызывает `verify`:
  - `PROGRAM_OPEN_IMPORT` — модуль вне программы импортирует модуль из `closed`;
  - `FEATURE_FLAG_MISMATCH` — §15.11.

  Каждая ошибка `verify` проваливает сборку. Сообщение называет модуль и предлагает добавить его в
  `program.include` или выключить `optimize`. `file` — id модуля вне программы, спан пустой.

### 15.4 Разрешение символов программы

- Экспорт модуля программы — одно из: локальная привязка (`export const/let/function/class`, `export { x }`,
  `export default x` с идентификатором), реэкспорт имени (`export { x as y } from`), реэкспорт всего
  (`export * from`), реэкспорт namespace (`export * as ns from`), `export default <выражение>` (не привязка,
  в решениях не участвует).
- `link` разрешает каждое импортированное имя в одно из: `(модуль, привязка)`, экспорт рантайма по имени,
  внешнее (модуль вне программы) или неразрешённое. `export *` работает по семантике ES: явные экспорты
  важнее, `default` не реэкспортируется, имя, которое дают два `export *`, не экспортируется. Циклы допустимы.
- Примитивы (§8.0) распознаются и через реэкспорты программы (`lib.ts: export { signal as s } from "reze-js"`),
  и через namespace: `R.signal(…)`, где `R` — namespace-импорт модуля рантайма или модуля программы,
  реэкспортирующего примитив. Namespace модуля рантайма распознаётся и в модульном режиме. Примитивы:
  `signal`, `computed`, `store` (§15.6); для §15.9 и §15.11 — `renderToString`, `hydrate`, `Suspense`.
- Использование привязки B — reference в модуле программы на B или на импорт, разрешённый в B, либо
  `ns.name`, разрешённое в B. Классы:
  - `Call0` — callee вызова без аргументов, не опционального, без type arguments (`x()`, `ns.x()`);
  - `Path(k₁…kₙ)`, `n ≥ 1` — rvalue-цепочка `x.k₁…kₙ` со статическими ключами (идентификатор или
    строковый литерал в `[]`), не опциональная, не callee, не цель присваивания, `delete` или update;
  - `Tag` — имя тега JSX;
  - `StoreSet` — вызов сеттера store в форме §15.6;
  - `Other` — всё остальное, включая ссылки в типах (`typeof x`), `export default x` и передачу как значения.

  Экспортные спецификаторы (`export { x }`, реэкспорты) — не использования, а рёбра разрешения.

- Модуль _открыт_, если: его namespace-объект (import или `export * as`) используется иначе, чем
  `ns.<статический ключ>`; его импортирует литеральный `import()`; под него подходит литеральный шаблон
  `import.meta.glob` (строка или массив строк, относительно импортёра). `import()` с нелитеральным
  аргументом и нелитеральный `import.meta.glob` открывают все модули программы.
- Экспортированная привязка _убегает_, если её модуль — вход или открыт; если её экспортирует под любым
  именем (через цепочку) входной или открытый модуль; если её импортирует модуль вне программы (это
  проверяет `verify`, §15.3). Убегающую привязку может использовать неизвестный код как угодно, поэтому
  межмодульных решений по ней нет.

### 15.5 Межмодульная свёртка сигналов (`optimize`)

Сигнал верхнего уровня модуля `M` сворачивается, если одновременно:

- выполнены условия O3, кроме запрета экспорта;
- геттер не убегает, и каждое его использование в каждом модуле программы — `Call0`;
- сеттер не убегает, и у него нет ни одного использования ни в одном модуле. Экспорт без импортёров допустим.

Переписывание:

- В `M` — как O3. Экспортные спецификаторы сеттера (`export { setX }`) удаляются;
  `export const [x, setX] = signal(v)` → `export const x = v`.
- В импортёрах `x()` → `x`, `ns.x()` → `ns.x`. Импорты не меняются: `x` по-прежнему экспортируется. Если
  `init` — литерал, чтение в JSX становится статическим текстом шаблона: §7.10 распространяется на
  импортированные свёрнутые чтения.
- `SIGNAL_FOLDED` с `data.scope: "program"` выдаётся в `M`; в `related` — использования в других модулях.

`SIGNAL_NOT_CALLED` (§7.2) в программном режиме распознаёт импортированные геттеры `signal`/`computed`.

### 15.6 store и снятие Proxy

Рантайм (новое в `@rezejs/signals`):

```ts
export function store<T extends object>(
  init: T,
): [state: T, setState: (fn: (draft: T) => void) => void];
```

- Семантика задана через сигналы: store ведёт себя так, как если бы у каждого собственного свойства
  каждого объекта и массива в дереве был свой `signal` с `Object.is`, плюс по сигналу на набор ключей и на
  `length`. `state` — глубокий Proxy только для чтения: чтение свойства читает его сигнал. Запись, `delete`
  и `defineProperty` через `state` бросают `TypeError`.
- `setState(fn)` вызывает `fn(draft)` под `untrack` и возвращает `undefined`. `draft` — изменяемый Proxy того
  же дерева. Запись свойства сразу пишет его сигнал и планирует сброс, как запись сигнала. Чтение возвращает
  текущее значение. После возврата `fn` draft-прокси недействительны: любое обращение бросает `TypeError`.
- Записанные объекты и массивы становятся частью дерева и оборачиваются лениво при чтении. `init` не копируется.

Снятие Proxy (`optimize`; проверяется по символам, при сомнении не применяется). `const [state, setState] = store(init)`
на любом уровне. В модульном режиме ни `state`, ни `setState` не экспортируются. В программном режиме
экспорт допустим, если привязки не убегают. Условия:

- `store` — примитив; нет аннотации типа и type arguments; паттерн — массив из одного или двух
  идентификаторов, как в O3;
- `init` — _форма_: объектный литерал без spread, методов, геттеров и сеттеров, вычисляемых ключей и ключа
  `__proto__`. Ключи — идентификаторы, строки или числа. Значение-объектный литерал — вложенная форма,
  любое другое значение, включая массив, — _лист_. Путь листа — ключи от корня;
- каждое использование `state` (§15.4, во всех модулях) — `Path` ровно до листа;
- каждое использование `setState` — `StoreSet`: вызов с одним аргументом, стрелкой или функциональным
  выражением (не async, не генератор) с ровно одним параметром-идентификатором `d` без значения по
  умолчанию. Каждая ссылка на `d` в теле — цепочка ровно до листа, которая стоит rvalue, целью `=`,
  составного или логического присваивания либо операндом `++`/`--`. Присваивания и update — только
  инструкции-выражения: значение не используется. На `d` нет ссылок во вложенных функциях. В теле нет
  `this`, `arguments`, `yield`, `await`.

Переписывание. Листья перечисляются в порядке исходника. Локальные имена даёт Namer: `<state>$<ключи через $>`
и `<setState>$<…>`, символы ключа, недопустимые в идентификаторе, заменяются на `_`. Имена экспортов
назначает `link`: `state$a`, `setState$a$b`, уникальные среди экспортов модуля.

- Объявление → `const [s$a, set$a] = signal(<a>), [s$b$c] = signal(<b.c>);`. Значения вычисляются по одному
  разу в исходном порядке. Сеттер листа объявляется, только если лист пишется.
- `state.a.b` → `s$a$b()`.
- `setState((d) => E)` → `void untrack(() => E')`; блочное тело → `void untrack(() => { … })`. В теле:
  - rvalue `d.p` → `s$p()`;
  - `d.p = e` → `set$p(() => e)`: форма-updater, чтобы функция записалась как значение;
  - `d.p op= e` → `set$p((v) => v op e)` для арифметических, битовых и логических (`||`, `&&`, `??`)
    операторов. Сигнал не уведомляет при равном значении, как и store;
  - `d.p++`, `++d.p` → `set$p((v) => ++v)`, `--` так же. `v` даёт Namer.
- Экспортированный store в программном режиме: `M` экспортирует листья, которые используют импортёры, под
  именами от `link`. Импортёр заменяет спецификатор `state`/`setState` в своём import на нужные листья
  (`import { state$a as _state$a } from "./store"`). Namespace-доступ (`ns.state.a`) запрещает снятие Proxy.
- `STORE_UNPROXIED` с `data.store` и `data.scope` выдаётся в модуле store; в `related` — использования.

Эквивалентность следует из определения store через сигналы: чтения идут в те же сигналы, запись в draft —
та же запись сигнала, а draft-чтения нетрекаемы, как под `untrack`.

### 15.7 Переписывание деструктуризации props (всегда)

Это правило семантики JSX (§7.8), а не оптимизация: оно не зависит от `optimize` и работает в обоих режимах.

Условия: функция — компонент (§7.8), у неё ровно один параметр, и это объектный паттерн, где:

- ключи статические (идентификатор, строка, число);
- значение свойства — идентификатор с необязательным значением по умолчанию (литерал по §7.10, `true`,
  `false`, `null`, `undefined`) или вложенный объектный паттерн по тем же правилам без значения по умолчанию;
- rest — только на верхнем уровне и только идентификатор;
- ни одна привязка паттерна не пишется; в теле нет `arguments`; функция не генератор.

Иначе — `PROPS_DESTRUCTURED` с `data.reason`: `computed-key`, `default`, `nested-default`, `nested-rest`,
`written`, `arguments`, `generator`, `params` (больше одного параметра), `array-pattern` (значение свойства —
массивный паттерн). Значение по умолчанию `undefined` равносильно его отсутствию.

Переписывание (аннотация типа параметра сохраняется):

- паттерн → `_props$` (Namer);
- ссылка на привязку с путём `k₁…kₙ` → `_props$.k₁…kₙ`; ключ, который не является идентификатором, → `["k"]`;
  со значением по умолчанию `d` → `(_props$.p === undefined ? d : _props$.p)`; сокращённое свойство `{ a }` →
  `{ a: _props$.a }`;
- rest `r` → `const r = splitProps(_props$, ["k", …])[1];` первой инструкцией тела, после директив. Тело-выражение
  стрелки → `{ const r = …; return (<тело>); }`;
- чтения становятся ленивыми: `_props$.a` — доступ к члену, то есть динамическое выражение (§7.11), и
  привязки JSX реактивны. Анализ компонентов (§15.8) видит компонент уже переписанным. Выдаётся
  `PROPS_REWRITTEN`.

### 15.8 Статические и клиентские компоненты

Компонент — функция верхнего уровня модуля программы (`function C`, `const C = …` со стрелкой или
функциональным выражением), чьё тело содержит JSX (§7.8). _Статический_ компонент при рендере даёт HTML и
ничего больше, а его DOM после рендера не меняется, поэтому на клиенте его выполнять не нужно. Классификация —
наибольшая неподвижная точка по всем компонентам программы (рекурсия допустима). C статический, если
одновременно:

1. C не async и не генератор; у него не больше одного параметра, и это идентификатор `p` (после §15.7);
   в теле нет `this`, `arguments`, `new.target`.
2. Тело — выражение, или последовательность `const`-объявлений идентификаторов с инертными
   инициализаторами, за которой идёт `return <инертное>`.
3. Каждое выражение инертно. Инертны:
   - литералы; шаблонные литералы, унарные (кроме `delete`), бинарные, логические и условные выражения,
     массивы и объекты (без spread, методов, геттеров и вычисляемых ключей) из инертных частей;
   - идентификаторы, разрешённые в `p`, в локальную `const` C с инертным инициализатором, в инертную
     константу программы или в параметр callback-а `map` (ниже);
   - чтения свёрнутых геттеров (`x()` по O3 или §15.5);
   - `p.k`: один уровень, статический ключ;
   - `x.k…` и `x[литерал]…`, где `x` — инертная константа или параметр callback-а `map`;
   - `x.map((item[, index]) => <инертное>)`, где `x` — инертная константа со значением-литералом массива,
     а callback — стрелка с телом-выражением;
   - JSX по п. 4.

   _Инертная константа_ — `const` верхнего уровня модуля программы с инертным инициализатором, в котором
   нет `p`, `p.k` и параметров `map`. Каждое её использование (§15.4, все модули) стоит в инертной позиции
   статического компонента или в инициализаторе другой инертной константы. Экспортированная инертная
   константа не должна убегать.

4. JSX: нативные элементы без `ref`, spread, событий (`on*`, `on:*`), `prop:*` и `value` на
   `<select>`/`<textarea>` (сервер не пишет их в HTML, §14.4), с инертными значениями атрибутов и детьми;
   фрагменты; компоненты программы, которые либо статические, либо клиентские в граничной позиции (§15.9).
   Компонент вне программы или компонент рантайма (`Show`, `For`, …) делает C клиентским.

Остальные компоненты — _клиентские_, с причиной (§15.10). Статический компонент, который вызывает
клиентский, выполняется на клиенте как обычно, поэтому статичность экономит код только там, где выше по
дереву нет клиентских компонентов (§15.9).

### 15.9 Острова (`islands`)

Граничная позиция — `<C …/>` в JSX статического компонента, где C — клиентский компонент программы и:

- C экспортирован из объявляющего модуля под именем E (реэкспорты не считаются);
- нет детей, атрибута `children`, spread и `ref`;
- значение каждого атрибута (голый атрибут — `true`) — JSON-форма: строка, конечное число,
  `true`/`false`/`null`, массив или объект из JSON-форм. Другой вариант — инертное выражение из `p.k`,
  инертной константы или параметра `map`; его сериализуемость проверяет рантайм (ниже).

Иначе позиция неграничная, и статический компонент становится клиентским (причина — эта позиция).
M4 расширяет это условие: слоты (§16.4) и `island:load` (§16.3).

Id острова — FNV-1a 64 в base36 от `<путь модуля C относительно config.root через "/">#E`. Он не зависит
от графа и target, поэтому SSR- и client-сборки согласны.

_Корень_ — вызов разрешённого примитива `renderToString(() => <R …/>)` или `hydrate(() => <R …/>, el)`, где
аргумент — стрелка без параметров, чьё тело — единственный элемент компонента R. R статический, а его
атрибуты — литеральные JSON-формы. Острова корня — граничные позиции, достижимые из R через статические
компоненты.

Server:

- корень → `renderToString(code, true)`: рендер начинается в _статической области_. Без `true` рендер
  начинается в _клиентской области_, и вывод совпадает с M2;
- граничная позиция → `ssrIsland("<id>", C, props)` вместо `createComponent`. В статической области
  `ssrIsland` открывает область ключей, как `createComponent` (§14.3), сериализует props и выводит
  `<!--$<id>:<ключ области>:<json>-->`, HTML компонента и `<!--/$-->`. Внутри острова область клиентская.
  В клиентской области (статический компонент под клиентским) `ssrIsland` ведёт себя как `createComponent`
  и маркеров не выводит;
- в JSON маркера `<`, `>`, `-` экранируются как `\u003c`, `\u003e`, `\u002d`. Сериализуемы: `null`, boolean,
  строка, конечное число, кроме `-0`, плотный массив, простой объект (прототип `Object.prototype` или
  `null`) с собственными перечислимыми строковыми ключами — рекурсивно и без циклов. Иначе
  `renderToString` бросает `Error("[ISLAND_PROPS] …")` с id острова и ключом props.

Hydrate:

- корень → `hydrateIslands(el, { "<id>": _$island0, … })`. `_$islandN` — импорты в шапке модуля:
  `import { E as _$island0 } from "<путь к модулю C относительно импортёра>"`. Импорт R в исходнике не
  трогается: R больше не используется, и бандлер удаляет его модуль, если у модуля нет побочных эффектов;
- статические компоненты и граничные позиции для hydrate и client компилируются как в M2 (`createComponent`):
  их код нужен, только когда их вызывает клиентский компонент;
- `hydrateIslands(el, islands)` для каждого маркера острова внутри `el` в порядке документа собирает
  `[data-hk]` между маркерами, вызывает `islands[id](props)` под `untrack` в области ключей
  `{ id: <ключ>, count: 0 }` и отдаёт результат вставке. `current` вставки — узлы между маркерами, якорь —
  закрывающий маркер, маркеры остаются в DOM. Функция возвращает функцию, которая освобождает все острова.
  Id без записи в `islands` → `Error`.

Эквивалентность: статические компоненты не читают реактивное состояние, не создают вычислений и не вешают
поведения (§15.8, п. 1–4), поэтому пропуск их выполнения на клиенте не наблюдаем. Props острова в
статической области не меняются, и остров получает те же значения, только простыми свойствами, а не геттерами.
`ISLAND` (info) выдаётся на граничной позиции с `data.id` и `data.features` (§15.11).

### 15.10 Причины

```rust
pub struct Reason { pub code: Code, pub module: String, pub span: Span, pub message: String, pub cause: Option<Box<Reason>> }
```

У каждого межмодульного решения и у каждого клиентского компонента есть `Reason`. Причина клиентского
компонента — первое в порядке исходника нарушение §15.8, п. 1–4 (например, атрибут `onClick`), либо
неграничная позиция клиентского компонента, и тогда `cause` — причина этого компонента. В программном
режиме `compile` выдаёт `STATIC_COMPONENT` и `CLIENT_COMPONENT` на объявлениях компонентов модуля:
`message` — первое звено, `related` — остальные. В модульном режиме классификации нет, и этих диагностик нет.

### 15.11 Фичи и define-флаги

Фича — экспорт рантайма. Фичи компонента — хелперы, которые emit для target `hydrate` использовал бы в его
JSX (`summarize` выполняет lower и собирает их), плюс экспорты рантайма, которые компонент использует.
Фичи острова — объединение фич клиентского кода, достижимого из C: клиентских компонентов и статических
компонентов, которые они вызывают. `data.features` — отсортированный список через запятую.

| Флаг                 | Значение                                                                   | Что убирает рантайм                        |
| -------------------- | -------------------------------------------------------------------------- | ------------------------------------------ |
| `__REZE_HYDRATION__` | target сборки `server` или `hydrate` (плагин: SSR-сборка или `hydratable`) | области ключей в `createComponent` (§14.3) |
| `__REZE_SUSPENSE__`  | в программе есть использование (§15.4) экспорта рантайма `Suspense`        | тело `trackPending`                        |

- Рантайм читает флаг только через `packages/dom/src/features.ts`:
  `export const Hydration = typeof __REZE_HYDRATION__ === "boolean" ? __REZE_HYDRATION__ : true;`. Без
  замены всё включено: тесты и сторонние сборщики работают как в M2.
- Плагин заменяет идентификаторы `__REZE_<NAME>__` литералами `true`/`false` в каждом модуле, чей код их
  содержит (`transform` с фильтром по коду). В serve и без программы все флаги `true`; `features` из опций
  включает флаг принудительно.
- `FEATURE_FLAG_MISMATCH` (`verify`): модуль вне программы использует экспорт рантайма, чья фича выключена
  флагом (например, `Suspense` при `__REZE_SUSPENSE__ = false`).
- Флаг добавляется только вместе со строкой этой таблицы и местом в рантайме. Флаг, который не уменьшает
  бандл в `benches/bundle-size`, не добавляется. Контекстов в рантайме нет, поэтому `__REZE_CONTEXT__` появится
  только вместе с ними.

### 15.12 IR и модули

IR остаётся независимым от target (§2). Дополнения к §6:

```rust
enum HoleKind<'a> {
    …,
    ConstSignalRead { getter: Span },                      // теперь и `ns.x()` → `ns.x` (§15.5)
    Specifiers { specifiers: Vec<'a, Specifier<'a>> },     // список спецификаторов import/export: Source(Span) | Alias { name, alias };
                                                           // замена спецификаторов store (§15.6), удаление `export { setX }` свёрнутого сеттера (§15.5)
    StoreDecl { leaves: Vec<'a, StoreLeaf<'a>> },          // StoreLeaf { getter, setter: Option, value: Embed }
    StoreExport { declaration: Embed<'a>, specifiers: Vec<'a, Specifier<'a>> }, // `export const [s, set] = store(…)` → объявление + `export { … }`
    StoreRead { getter: &'a str },
    StoreSet { body: Embed<'a> },                          // body — выражение или блок со скобками
    StoreWrite { setter: &'a str, write: StoreWriteKind<'a> }, // Assign(Embed) | Compound(op, Embed) | Update(op)
    PropsParam { name: &'a str },
    PropsRead { props: &'a str, path: Vec<'a, &'a str>, default: Option<Embed<'a>>, shorthand: bool },
    PropsRest { props: &'a str, binding: Span, keys: Vec<'a, &'a str>, body: Option<Embed<'a>> }, // body: тело-выражение стрелки
    ComputedInline { body: Embed<'a> },                    // O4: `d()` → `(expr)`
    Remove,                                                // O4: объявление удаляется
    IslandRoot { kind: RootKind, callee: Span, code: Embed<'a>, element: Option<Embed<'a>>, islands: Vec<'a, IslandImport<'a>> },
}
struct Component<'a> { callee: Embed<'a>, props: Props<'a>, island: Option<&'a str> } // callee: `<Tag/>` из props → `_props$.Tag`; id: server → `ssrIsland`
```

`IslandRoot`: server → `renderToString(code, true)`; hydrate → `hydrateIslands`; client → исходник как есть.

| Модуль            | Отвечает за                                                                                          |
| ----------------- | ---------------------------------------------------------------------------------------------------- |
| `summary.rs`      | `ModuleSummary`, `summarize`: экспорты, импорты, использования, сигналы, store, компоненты, фичи     |
| `link.rs`         | `link`: разрешение, открытые модули, убегание, свёртки, неподвижная точка компонентов, id островов   |
| `verify.rs`       | `verify` (§15.3)                                                                                     |
| `facts.rs`        | `ModuleFacts`: решения для одного модуля и их `Reason`; `analyze` читает их вместо модульных выводов |
| `features.rs`     | таблица флагов §15.11                                                                                |
| `lower/props.rs`  | §15.7                                                                                                |
| `lower/store.rs`  | §15.6                                                                                                |
| `lower/island.rs` | корни и граничные позиции (§15.9)                                                                    |

### 15.13 Тесты

- `tests/programs/<name>/…` — многомодульные фикстуры. Каждая проходит `summarize` → `link` → `compile`
  всех модулей для трёх target; insta-снапшот — все выводы и info-диагностики. Покрытие: реэкспорт
  примитива через barrel и namespace; экспортированный сигнал, свёрнутый и несвёрнутый (сеттер импортирован,
  геттер убегает через вход, открытый модуль, `import.meta.glob`); store локальный и экспортированный плюс
  каждое условие отказа; каждый случай §15.7; каждое правило §15.8, п. 1–4; граница острова и каждое условие
  отказа; корни; рекурсивные компоненты.
- `compile.rs`: `FACTS_STALE`; инвариант одного модуля (§15.2) для всех снапшот-входов; регрессия M1 об
  экспорте в O3.
- `verify`: Rust-тесты на `PROGRAM_OPEN_IMPORT` и `FEATURE_FLAG_MISMATCH` (каталог требует теста на каждый код).
- `packages/dom/tests`: фикстура программы собирается Vite в build и компилируется без фактов, DOM после
  каждого шага совпадает. Store: сценарии с Proxy (`optimize: false`) и без него совпадают. Острова:
  `renderToString` с `true` и без него даёт одинаковый HTML после удаления маркеров `<!--$…-->`/`<!--/$-->`;
  после каждого события DOM с гидратацией островов совпадает с M2-гидратацией; функция в props острова даёт
  `[ISLAND_PROPS]`.
- `benches/bundle-size`: пример `examples/islands` (статическая страница с одним счётчиком-островом).
  Критерий: client-бандл с `islands: true` меньше, чем с `islands: false`; каждый флаг §15.11 уменьшает бандл.

### 15.14 Изменения рантайма и пакетов (M3)

- `@rezejs/signals`: `store`.
- `@rezejs/dom`: `features.ts`; `createComponent` и `trackPending` за флагами; `ssrIsland`,
  `renderToString(code, islands?)`, `hydrateIslands`. `reze-js` реэкспортирует новое.
- `@rezejs/vite-plugin`: опции `program`, `islands`, `features`; сканирование в `buildStart`; замена флагов;
  `verify` в `buildEnd`; `islands` без `hydratable` — ошибка конфигурации.
- `@rezejs/compiler` (`index.d.ts`): `summarize`, `link`, `verify`, опция `facts`, `Diagnostic.related`.
- `reze_napi`: функции §15.2. SKILL.md перегенерирован с новыми кодами.

### 15.15 Границы M3

Переехали в M4 (§16): анализ программы в dev и HMR; ленивые чанки на остров; дети и слоты у островов;
межмодульный O4; снятие Proxy с массивов и записи не в лист; значения по умолчанию не литералы в §15.7;
стриминг.

## 16. Программа в dev, ленивые острова, слоты и остатки M3 (M4)

Инварианты §15 действуют и здесь: решения M4 только добавляют свёртки или откладывают уже решённое
выполнение. Видимый DOM и поведение после гидратации не меняются, кроме явно помеченного: события,
пришедшие в ленивый остров до его загрузки, теряются (§16.3, opt-in через атрибут).

### 16.1 Режимы M4

| Возможность          | dev-serve                                    | build                    |
| -------------------- | -------------------------------------------- | ------------------------ |
| Анализ программы     | только с `program.dev: true` (§16.2)         | как в §15.3              |
| Острова              | те же решения, что в build при тех же входах | §15.9                    |
| `island:load`, слоты | работают в обоих                             | §16.3, §16.4             |
| Стриминг             | — (только `renderToString`)                  | `renderToStream` (§16.8) |

### 16.2 Анализ программы в dev и HMR

Опция плагина: `program.dev?: boolean` (по умолчанию `false`). Без неё serve работает в модульном режиме.

- При старте сервера плагин сканирует программу как в `buildStart` (§15.3), но `is_entry` — модули, в
  которые разрешились модульные скрипты всех `*.html` под `config.root`. Дальше `link`; результат хранится
  в памяти.
- Изменение/добавление/удаление файла (вотчер): файл перечитывается и проходит `summarize` заново,
  затронутые спецификаторы разрешаются заново, затем полный `link` (дёшево: сводки уже построены).
- Несовпадение хеша в `transform` в dev — не `FACTS_STALE`: `transform` перечитывает файл с диска,
  перестраивает программу синхронно и компилирует с новыми фактами.
- HMR. Пусть `A` — изменившийся модуль, `D(A)` — модули программы, транзитивно импортирующие `A` (по
  разрешённым рёбрам `link`). Инвалидируются через `moduleGraph` `A ∪ D(A)`. Полный reload
  (`server.ws.send({ type: "full-reload" })`) вместо HMR, если: `A` или любой модуль из `D(A)` входит в
  `closed`, является границей острова или корнем, либо его классификация (§15.8) изменилась. Иначе —
  обычный HMR-апдейт затронутых модулей: факты остальных не меняются, поведение сохраняется.
- Замкнутый мир в dev: `transform` модуля вне программы, импортирующего модуль из `closed`, бросает ту же
  ошибку, что `PROGRAM_OPEN_IMPORT` (оверлей). Флаги в dev всегда `true` (§15.11): mid-сессионное
  выключение флага потребовало бы полного reload всех потребителей рантайма.

### 16.3 Ленивые острова

Граничная позиция (§15.9) может нести `island:load="eager" | "idle" | "visible" | "interaction"`
(по умолчанию `eager` — семантика M3, остров гидратируется синхронно). Атрибут снимается на границе и в
props не попадает. Вне граничной позиции и в сборке без `islands` атрибут даёт `ISLAND_DIRECTIVE_IGNORED`
(warn) и компилируется как есть.

- Server: режим пишется в маркер острова: `<!--$<id>:<ключ>:<json>:<mode>-->`, где `mode` — `eager`, `idle`,
  `visible` или `interaction`.
- Hydrate: корень → `hydrateIslands(el, islands)`, где значение — компонент (eager, импорт статически, как
  в §15.9) или дескриптор `{ load: () => import("<путь>"), mode, export: "<E>" }`. Путь — относительный путь
  к модулю C от импортёра, литерал: Rolldown делит чанк по `import()`. `load()` резолвит namespace модуля,
  компонент берётся по `export`.
- Загрузка: `idle` — `requestIdleCallback` (fallback `setTimeout(…, 1)`); `visible` — `IntersectionObserver`
  за ближайшим элементным соседом открывающего маркера, иначе за родителем, disconnect после срабатывания;
  `interaction` — `pointerdown`/`focusin`/`keydown` (capture) на родителе открывающего маркера. Первое
  взаимодействие внутри диапазона острова запускает его загрузку немедленно при любом режиме. События,
  пришедшие до завершения загрузки, теряются без replay: режим включается только явным атрибутом.
- Возвращаемая `hydrateIslands` функция освобождения отменяет и незавершённые загрузки. Выдаётся
  `LAZY_ISLAND` (info) с `data.id` и `data.mode`; `ISLAND` выдаётся тоже, с тем же `data.mode`.

### 16.4 Дети и слоты у островов

Условие границы (§15.9) расширено: допускаются вложенные дети (= слот `children`) и атрибуты со значением
JSX-формы (= именованные слоты). JSX-форма: элемент, фрагмент, массив и текст из инертного JSX
статического родителя; значение-функция (render props) границу запрещает (причина — эта позиция). Остальные
значения — по-прежнему JSON-формы с проверкой рантаймом. `children`-атрибут плюс вложенные дети — по-прежнему
`CHILDREN_PROP_IGNORED`, побеждают вложенные.

- Server: родитель рендерит каждый слот в HTML-строку в своей области ключей; позиция →
  `ssrIsland("<id>", C, props, slots)`, где `slots: Record<string, string>`. Внутри C вставка значения
  `props.<имя>` (только позиция вставки, §7.5) выводит `<!--$slot:<имя в percent-encoding>-->`, HTML слота и
  `<!--/$slot-->`. Сервер вычисляет слот в каждой позиции заново — как геттер `children` в M2. Использование
  слота вне позиции вставки (условия, сравнение, передача дальше как значения) запрещает границу: серверная
  строка и клиентский массив узлов расходятся (`""` ложно, а `[]` истинно). Причина — это использование.
- Hydrate: значение слота — массив узлов из всех его диапазонов по порядку, пустой слот — `undefined` (как
  отсутствующие `children` в M2). Якорь вставки — закрывающий маркер диапазона; дальше обычные семантики
  `insert`/`claimInsert`, включая move-семантику при вставке одного значения дважды. Поиск маркеров идёт по
  живому DOM: HTML слотов — обычный живой HTML, вложенные острова внутри слотов гидратируются общим проходом
  в порядке документа, независимо от того, вставил ли внешний остров слот.
- Корень (§15.9) без изменений: его дети — обычное статическое дерево, слотами не являются.

### 16.5 Межмодульный O4 (`optimize`)

`const d = computed(() => expr)` верхнего уровня модуля `M` инлайнится в место чтения в другом модуле, если
одновременно:

- выполнены условия O4 (§8), кроме «в той же функции»: чтение — единственное использование `d`
  (§15.4) во всей программе, `Call0` в динамическом выражении JSX (bind, insert, getter prop);
- `expr` ссылается только на привязки верхнего уровня `M`; каждая из них импортируется в читающий модуль
  (существующим импортом или добавленным спецификатором; конфликты имён решает Namer);
- добавление ребра не создаёт импортный цикл между `M` и читающим модулем; `d` не убегает иначе.

Переписывание: объявление и его экспортные спецификаторы в `M` удаляются, `d()` → `(expr)` с импортированными
привязками. Временная семантика сохраняется через живые привязки ES-импортов: `computed` ленив, и чтение на
месте использования видит те же значения. Выдаётся `COMPUTED_INLINED` с `data.scope: "program"` в `M`;
в `related` — место чтения.

### 16.6 store: массивы и записи не в лист (`optimize`)

Расширяет §15.6. Массивы в форме — тоже листья. Дополнительно к правилам §15.6:

- Чтение массива целиком — только: `each` у `<For>`, spread `[...state.a]`, `.length`, индексное чтение
  `state.a[i]` как rvalue. Методы массива — только инструкциями-выражениями (возврат игнорируется).
  Элементы и их поля — `Path`-rvalue и вставки в JSX/биндинги; передача элемента целиком в `===`, вызовы и
  spread элемента-объекта запрещают снятие (причина — это использование).
- Запись: `d.a[i] = e`, `d.a[i].p… = e` (путь вглубь — только литеральные ключи), методы-инструкции
  (`push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`, `copyWithin`, `fill`) — через copy-on-write
  updater: `set$a((v) => { const c = v.slice(); …; return c; })`. Нетронутые элементы сохраняют идентичность
  (`slice`), поэтому keyed-`For` ведёт себя как с Proxy. `delete` запрещён везде.
- Запись в путь формы (не лист) объектным литералом без spread раскладывается в по-листовые записи в порядке
  листьев; значения вычисляются по одному разу в исходном порядке во временные. Запись не-литерала в форму —
  отказ.

Эквивалентность: каждая запись — одна запись сигнала; читатели видят те же значения в том же порядке сброса,
как и через Proxy.

### 16.7 Не-литеральные значения по умолчанию в props (всегда)

Расширяет §15.7: значение по умолчанию — чистое выражение: литералы, идентификаторы, доступы к членам,
стрелки и функциональные выражения, шаблонные литералы без тегов, унарные/бинарные/логические/условные и
массивы/объекты из чистых частей. Вызовы, `new`, tagged templates, `await`/`yield` — не чистые, и тогда
`PROPS_DESTRUCTURED` с `data.reason: "default"`.

Чистый default вычисляется один раз первым стейтментом тела (после `splitProps` при наличии rest) во
временную Namer-а; чтения используют временную. Время вычисления совпадает с оригиналом (вход в компонент),
поэтому семантика сохраняется, а чтения остаются ленивыми через `_props$`.

### 16.8 Стриминг

Рантайм (`@rezejs/dom/server`, новое):

```ts
export function renderToStream(
  code: () => JSX.Element,
  options?: { timeoutMs?: number },
): ReadableStream<Uint8Array>;
```

- Шелл стримится сразу, синхронно: async-компоненты дают состояние до загрузки (как `renderToString`, §14.4).
  Каждый незавершённый async-компонент — плейсхолдер `<!--$s:<id>--><!--/$s-->`, где `id` — ключ области
  компонента (§14.3) плюс индекс await. `Suspense` на сервере всегда показывает children; стрим дополняет их
  чанками.
- Как промис резолвится, сервер перерендеривает границу с закешированными значениями await и стримит чанк
  `<template data-reze-chunk="<id>" data-reze-values="<json>">…html…</template>`. Значения сериализуются тем
  же сериализатором, что props островов (§15.9); несериализуемое значение бросает `Error("[STREAM_VALUES] …")`.
  Чанки не зависят от порядка: каждый несёт свой `id`. Инлайн-скриптов нет.
- Стрим закрывается, когда всё завершено или истёк `timeoutMs` (по умолчанию 30000): незавершённые границы
  остаются фолбэком. Обрыв соединения даёт частичный HTML с фолбэками вместо недостающих чанков.
- Клиент (`hydrate`/`hydrateIslands`): перед клеймом каждый `template[data-reze-chunk]` заменяет содержимое
  своего плейсхолдера; async-компонент сначала читает значения из карты чанков и при наличии не загружает
  заново, иначе — обычная загрузка. Собранный DOM совпадает с `renderToString` после резолва всех промисов
  при тех же значениях.
- Компилятор: для target `server` каждый await верхнего уровня async-компонента дополнительно вызывает
  `ssrAwait(boundaryKey, i, () => e)`; вне стрима регистрации игнорируются. Для target `hydrate` загрузка
  идёт через `streamValue(boundaryKey, i, loader)`. IR тот же (§14), emit различается только этим.

### 16.9 Диагностика

Новое: `LAZY_ISLAND` (info), `ISLAND_DIRECTIVE_IGNORED` (warn, исправление — удалить атрибут), `data.mode`
у `ISLAND`, `data.scope: "program"` у `COMPUTED_INLINED` (см. §9.4).

### 16.10 Тесты

- `tests/programs/<name>/…`: фикстуры на каждый пункт §16.2–§16.8 (dev-режим эмулируется тем же пайплайном
  `summarize → link → compile` с входами из HTML; инвариант: решения совпадают с build при тех же входах).
- `packages/dom/tests`: стрим собирается целиком и сравнивается с `renderToString` после резолва и с client;
  обрыв стрима оставляет фолбэки; `[STREAM_VALUES]` на функции; слоты: DOM после каждого шага совпадает с M2;
  двойная вставка слота делит move-семантику M2; ленивый остров: события до загрузки теряются, после — работают.
- `benches/bundle-size`: пример с ленивым островом — чанк острова отделён от шелла.

### 16.11 Изменения рантайма и пакетов (M4)

- `@rezejs/dom`: `renderToStream`, `ssrAwait`, `streamValue`, подмена чанков в `hydrate`/`hydrateIslands`,
  дескрипторы `{ load, mode, export }`, слот-маркеры в `ssrIsland` и значения слотов при гидратации.
- `@rezejs/vite-plugin`: `program.dev`, HMR-инвалидация и full-reload, ошибка замкнутого мира в dev,
  `islands` без `hydratable` — по-прежнему ошибка конфигурации.
- `@rezejs/compiler` (`index.d.ts`): новых функций нет; SKILL.md перегенерирован с кодами §16.9.

### 16.12 Границы M4

Вне M4: replay событий, пришедших до загрузки ленивого острова; слоты-функции (render props через границу);
межмодульный O4 через циклы; server actions и мутации; глобальный режим ленивости по умолчанию; nonce/CSP для
стриминга не нужен (скриптов нет) — но это следствие дизайна, а не фича.

## 17. LSP (M99)

Крейт `reze_lsp`, бинарь `reze-lsp`: LSP поверх stdio. Pure-логика (`analysis`, `mapping`) отделена от
транспортного цикла (`server`) и покрыта тестами; цикл проверяется живой сессией.

### 17.1 Программа

Один открытый документ компилируется в модульном режиме (§15.1). Два и больше — как программа: каждый файл
проходит `summarize`, спецификаторы разрешаются среди открытых и файлов под корнем workspace (рекурсивно,
кроме `node_modules` и каталогов на `.`; расширения `.ts/.tsx/.js/.jsx/.mts/.cts/.mjs/.cjs`), затем `link`,
затем каждый модуль компилируется со своими фактами. Открытые буферы перекрывают диск. `is_entry: false`
для всех: LSP только объясняет, ничего не переписывает. Инвариант §15.2 сохраняется: один файл линкуется
в то же, что модульный режим.

Настройки (`initializationOptions`, затем `workspace/didChangeConfiguration`): `moduleName` (по умолчанию
`"reze-js"`), `optimize` (`true`), `islands` (`true`), `root` (по умолчанию корень workspace).

### 17.2 Диагностика

Каждая диагностика компилятора (§9.2), включая `info`, отдаётся как есть дважды: push
(`textDocument/publishDiagnostics`) и pull (`textDocument/diagnostic`). Отображение: severity
`error/warn/info` → `Error/Warning/Information`; `line - 1`, колонка UTF-16 без пересчёта; `code` — имя кода;
`codeDescription` — `docs` (§9.2); `source` — `"reze"`; `relatedInformation` — `labels` (тот же файл) и
`related` (цепочка `Reason`, §15.10): спаны чужих файлов разрешаются по открытым текстам, иначе нулевой
range; `data` — полный JSON `file/path/labels/related/fixes/data/docs/rendered`. `fixes` дополнительно
отдаются как `textDocument/codeAction` (`quickfix`, `isPreferred: true`, заголовок
`"<fix> (<CODE>)"`).

### 17.3 Inlay hints

Каждая `info`-диагностика — один hint в конце её спана (`textDocument/inlayHint`): `SIGNAL_FOLDED`
→ `folded (<scope>)`, `COMPUTED_INLINED` → `inlined (<scope>)`, `STORE_UNPROXIED` → `store: signals (<scope>)`,
`DEAD_BRANCH_REMOVED` → `dead branch removed`, `PROPS_REWRITTEN` → `props: reactive reads`,
`STATIC_COMPONENT` → `static`, `CLIENT_COMPONENT` → `client: <reason, 64 символа>`, `ISLAND` →
`island <id> [<features>] [(<mode>)]`, `LAZY_ISLAND` → `lazy: <mode>`. Tooltip — `message` диагностики.
Остальные коды hint не дают.

### 17.4 «Почему остров?»

`textDocument/hover` и запрос `reze/whyIsland { uri, offset }` объясняют диагностику под курсором
(приоритет `ISLAND`/`LAZY_ISLAND`, затем `STATIC`/`CLIENT_COMPONENT`, затем остальные; из нескольких —
самая узкая). Markdown: сообщение, строка `in`, для острова — `id`, `mode`, `features` и почему позиция
стала границей; для клиентского — нумерованная цепочка `Reason` (`message` плюс `related` с файлами);
затем таблица `data`, заголовки `fixes` и ссылка на руководство. `hover` дополнительно возвращает range
диагностики.

### 17.5 Границы M99

Вне M99: rename/переезд по `related`, replay ленивых событий, workspace-диагностика, семантические токены,
конфигурация через файлы.
