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
  (§8) и снятия Proxy со store.

## 2. Этапы

| Этап | Содержание | Статус |
|---|---|---|
| M1 | Новая архитектура (§4), target `Client`, система диагностики (§9), помодульные оптимизации O1–O3, O5 (§8), исправление багов v0 (§11) | готово |
| **M2** | Targets `Server` и `Hydrate` на том же IR (§14) | готово |
| M3 | Анализ программы: `ModuleSummary` → `Facts`, острова, фичи на остров, межмодульная свёртка сигналов, снятие Proxy со store, define-флаги рантайма | потом |
| M4 | `reze_lsp`: диагностика, inlay hints, «почему остров?» по цепочкам `Reason` | потом |

Требования M1 ради M2–M4: IR не содержит ничего специфичного для client (выбор target делается только в
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

- `Ok(None)`: в файле нет JSX. Вызывающая сторона оставляет исходник как есть.
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

| Модуль | Отвечает за |
|---|---|
| `lib.rs` | `compile`, `Options`, `Output` |
| `diagnostic/mod.rs` | `Diagnostic`, `Severity`, `Fix`, `Label`, сбор, рендер в текст и JSON; `LineIndex` |
| `diagnostic/catalog.rs` | единый каталог кодов (§9.4): severity, заголовок, объяснение, исправление, пример «плохо/хорошо» |
| `code.rs` | `Code`: буфер вывода + метки `(generated, source)`; source map v3 |
| `html.rs` | таблицы элементов/атрибутов/событий, экранирование HTML и JS, очистка JSX-текста, сущности |
| `analyze.rs` | распознавание примитивов рантайма по символу (§8.0), O3, геттеры сигналов для `SIGNAL_NOT_CALLED` |
| `ir.rs` | IR (§6) |
| `lower/mod.rs` | `Lowerer`, поиск дыр в JS-участке (`Embed`), путь компонента для диагностики |
| `lower/element.rs` | нативные элементы → `Template` (HTML, дерево узлов, обходы, операции) |
| `lower/attribute.rs` | классификация атрибутов (§7.2), статическая свёртка |
| `lower/component.rs` | компоненты, props, дети компонентов |
| `lower/children.rs` | списки детей: правила JSX-текста, O5, вставки, условия, массивы детей |
| `lower/constant.rs` | статическое вычисление (`static_text`, истинность), `is_dynamic` |
| `lower/async_component.rs` | план async-компонента (§7.9) |
| `emit/mod.rs` | `Emitter`, склейка дыр, `Namer`, импорты `Helper`, таблица шаблонов (фабрики или строки server), шапка и хвост модуля, выбор эмиттера шаблона по target |
| `emit/template.rs` | client и hydrate: IIFE шаблона (клон или `claim`), обходы, операции, слитый `bind` |
| `emit/server.rs` | server: HTML шаблона, разрезанный по динамическим частям, и `ssr` (§14.1) |
| `emit/component.rs` | `createComponent`, объекты props, `mergeProps`, дети |
| `emit/async_component.rs` | синхронная форма async-компонентов |

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

struct Component<'a> { callee: Span, props: Props<'a> }
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

Правила проверяются по порядку. Результат зависит от значения: *литерал* (строка, число, `true`,
`false`, `null`, `undefined` или свёрнутая константа, §7.10), *статическое* (не реактивное по §7.11)
или *динамическое*.

| Атрибут | Литерал | Статическое | Динамическое |
|---|---|---|---|
| `ref` | — | `Op::Ref` | `Op::Ref` |
| `children` | используется как дети, если вложенных детей нет; иначе игнорируется + `CHILDREN_PROP_IGNORED` | | |
| `on:<name>` | строка → HTML-атрибут `on<name>` | `addEventListener(el, name, h)` | то же |
| `on<Upper>…` | строка → HTML-атрибут | делегированное или прямое (§7.4) | то же |
| `on<lower>…` с не-строковым значением | `EVENT_NAME_LOWERCASE`, компилируется как `on<Upper>` | | |
| `class`, `className`, `classList` | §7.3 | | |
| `style` | строка → атрибут; объект из одних литералов → свёрнутый `a:b;c:d` | `style(el, v)` | bind `style(el, v, prev)` |
| `prop:<n>` | `el.n = v` | `el.n = v` | bind |
| `attr:<n>` | атрибут | `setAttribute` | bind |
| `bool:<n>` | есть, если истинно | `setBoolAttribute` | bind |
| `xlink:*`, `xml:*` | атрибут | `setAttributeNS` | bind |
| `value`, `checked`, `selected` (только HTML) | атрибут шаблона, **кроме** `value` на `<textarea>`/`<select>` — там `Set` свойства | `el.n = v` | bind |
| `textContent`, `innerHTML` (только HTML) | `el.n = v` | `el.n = v` | bind |
| `key` | атрибут + `KEY_ON_ELEMENT` | | |
| остальное | атрибут (`true` → `name="true"`, голый → `name`, `false`/`null`/`undefined` → опущен) | `setAttribute` | bind |

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
- Деструктуризация props в параметрах компонента (`function C({ a })`, где `C` возвращает JSX) → `PROPS_DESTRUCTURED`
  (warn): чтение происходит один раз в теле и теряет реактивность. Автоматическое переписывание — M3.
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
(`import * as R`) и реэкспорты через другие модули — M3.

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

Переписывание: декларатор → `get = init` (инициализатор вычисляется один раз, как раньше), каждое
`get()` → `get`. Если `init` — литерал по §7.10, чтения в JSX становятся статическим текстом шаблона.
Пример: `const [title] = signal("Reze"); <h1>{title()}</h1>` → `const title = "Reze";` и шаблон `<h1>Reze</h1>`.

### O4. Инлайн `computed` (M3, отложено)
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

### Позже (M3)
Межмодульная свёртка сигналов, статические компоненты без клиентского кода, фичи рантайма на остров +
define-флаги (`__REZE_CONTEXT__`…), снятие Proxy со store, переписывание деструктуризации props, O4.

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
  code: string;                         // стабильный SCREAMING_SNAKE, никогда не переиспользуется
  severity: "error" | "warn" | "info";
  message: string;                      // "[CODE] …" по §9.1
  file: string;
  start: { offset: number; line: number; column: number };   // line 1-based, column 0-based UTF-16
  end:   { offset: number; line: number; column: number };
  path: string[];                       // ["<App>", "<TodoList>", "li", "button"]: компоненты и элементы от корня функции
  labels: { start: number; end: number; message: string }[];   // вторичные спаны (например, первый дубликат)
  fixes: { title: string; edits: { start: number; end: number; text: string }[] }[];  // применимы как есть
  data: Record<string, string>;         // структурированные факты (имя атрибута, причина…)
  docs: string;                         // https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md#<code в нижнем регистре>
  rendered: string;                     // текстовый рендер §9.3 без подвала: единственный рендерер — в Rust
}
```
Rust: `Diagnostic { code: Code, severity, span, labels, fixes, data, path }`. `Code` — enum каталога. Позиции
вычисляются одним `LineIndex` в конце компиляции.

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

| Код | Severity | Триггер | Исправление (`fixes`) |
|---|---|---|---|
| `PARSE_ERROR` | error | диагностика `oxc_parser` | — |
| `CLASS_ALIAS` | warn | `className`/`classList` на нативном элементе или в литеральном атрибуте со spread | переименовать в `class` |
| `CHILDREN_PROP_IGNORED` | warn | атрибут `children` и вложенные дети одновременно | удалить атрибут |
| `KEY_ON_ELEMENT` | warn | `key` на нативном элементе | удалить атрибут |
| `DUPLICATE_ATTRIBUTE` | warn | одно имя дважды на элементе | удалить ранний (label на нём) |
| `UNKNOWN_ATTRIBUTE` | warn | почти-совпадение с известным атрибутом | переименовать в подсказку |
| `EVENT_NAME_LOWERCASE` | warn | `onclick={fn}` — не-строковое значение у `on<lower>` | переименовать в `onClick` |
| `SIGNAL_NOT_CALLED` | warn | геттер сигнала без вызова в атрибуте/свойстве/style | `count` → `count()` |
| `PROPS_DESTRUCTURED` | warn | деструктуризация props в параметрах компонента | — (описание в SKILL) |
| `INLINE_EACH` | warn | `<For each={[…]}>` | — |
| `ASYNC_COMPONENT_SHAPE` | warn | async-компонент вне поддерживаемой формы; `data.reason` | — |
| `ASYNC_RETURN_TYPE` | warn | аннотация `Promise`, которую нельзя развернуть | — |
| `SIGNAL_FOLDED` | info | применена O3; `data.signal` | — |
| `DEAD_BRANCH_REMOVED` | info | применена O5 | — |

### 9.5 Каналы
- `Output.diagnostics` / napi: полный JSON по §9.2, включая `info` (это объяснения оптимизатора для LSP в M4).
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
