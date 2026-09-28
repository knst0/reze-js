(() => {
  const observed = { subtree: true, childList: true, attributes: true, characterData: true };
  const timeoutMs = 10_000;

  const until = (isDone, label) =>
    new Promise((resolve, reject) => {
      if (isDone()) return resolve(performance.now());
      const observer = new MutationObserver(() => {
        if (!isDone()) return;
        observer.disconnect();
        clearTimeout(timer);
        resolve(performance.now());
      });
      const timer = setTimeout(() => {
        observer.disconnect();
        reject(new Error(`${label} timed out`));
      }, timeoutMs);
      observer.observe(document, observed);
    });

  const painted = () => new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));

  const rows = () => document.querySelector("tbody")?.children;
  const count = () => rows()?.length ?? 0;
  const idAt = (i) => rows()[i].children[0].textContent;
  const labelAt = (i) => rows()[i].children[1].textContent;
  const click = (selector) => document.querySelector(selector).click();

  async function reset1k() {
    const before = count() ? idAt(0) : null;
    click("#run");
    await until(() => count() === 1000 && idAt(0) !== before, "reset1k");
  }

  async function empty() {
    if (count() === 0) return;
    click("#clear");
    await until(() => count() === 0, "empty");
  }

  const ops = {
    create: {
      setup: empty,
      act() {
        click("#run");
        return () => count() === 1000;
      },
    },
    replace: {
      setup: reset1k,
      act() {
        const before = idAt(0);
        click("#run");
        return () => count() === 1000 && idAt(0) !== before;
      },
    },
    update: {
      setup: reset1k,
      act() {
        const before = labelAt(0);
        click("#update");
        return () => labelAt(0) !== before;
      },
    },
    select: {
      setup: reset1k,
      act() {
        click("tbody > tr:nth-child(2) > td:nth-child(2) > a");
        return () => rows()[1].classList.contains("danger");
      },
    },
    swap: {
      setup: reset1k,
      act() {
        const a = idAt(1);
        const b = idAt(998);
        click("#swaprows");
        return () => idAt(1) === b && idAt(998) === a;
      },
    },
    remove: {
      setup: reset1k,
      act() {
        const before = idAt(3);
        click("tbody > tr:nth-child(4) > td:nth-child(3) > a > span");
        return () => count() === 999 && idAt(3) !== before;
      },
    },
    createLots: {
      setup: empty,
      act() {
        click("#runlots");
        return () => count() === 10000;
      },
    },
    append: {
      setup: reset1k,
      act() {
        click("#add");
        return () => count() === 2000;
      },
    },
    clear: {
      setup: reset1k,
      act() {
        click("#clear");
        return () => count() === 0;
      },
    },
  };

  window.__bench = {
    ready: until(() => document.querySelector("#run") !== null, "boot"),
    async prepare(op) {
      await ops[op].setup();
      await painted();
    },
    /** Performs `op` and returns ms from the click until its DOM commit. */
    async run(op) {
      const start = performance.now();
      const end = await until(ops[op].act(), op);
      await painted();
      return end - start;
    },
    async cycle(n) {
      for (let i = 0; i < n; i++) {
        await reset1k();
        await empty();
      }
    },
  };
})();
