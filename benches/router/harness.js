(() => {
  const observed = { subtree: true, childList: true, attributes: true, characterData: true };

  const isShown = (selector, text) => {
    const el = document.querySelector(selector);
    return el !== null && (text === undefined || el.textContent === text);
  };

  const until = (selector, text) =>
    new Promise((resolve) => {
      if (isShown(selector, text)) return resolve(performance.now());
      const observer = new MutationObserver(() => {
        if (!isShown(selector, text)) return;
        observer.disconnect();
        resolve(performance.now());
      });
      observer.observe(document, observed);
    });

  const user = (id) => until(`[data-page="user"][data-id="${id}"] h2`, `User ${id}`);
  const page = (name) => until(`[data-page="${name}"]`);

  function click(selector, shown) {
    document.querySelector(selector).click();
    return shown;
  }

  const steps = {
    param() {
      const next = Number(document.querySelector('[data-page="user"]').dataset.id) + 1;
      return click("a[data-next]", user(next));
    },
    swap(i) {
      return i % 2 === 0 ? click('nav a[href="/about"]', page("about")) : click('nav a[href="/users/1"]', user(1));
    },
    pop(i) {
      const shown = i % 2 === 0 ? page("about") : user(1);
      if (i % 2 === 0) history.back();
      else history.forward();
      return shown;
    },
    home() {
      return click('nav a[href="/"]', page("home"));
    },
  };

  window.__bench = {
    boot: until("[data-page]"),
    user,
    page,
    /** Runs `count` steps of `scenario`, each awaiting its DOM commit; returns elapsed ms. */
    async run(scenario, count) {
      const start = performance.now();
      for (let i = 0; i < count; i++) await steps[scenario](i);
      return performance.now() - start;
    },
  };
})();
