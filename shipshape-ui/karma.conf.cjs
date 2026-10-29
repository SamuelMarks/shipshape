function createJasmineZoneCompatPreprocessor() {
  return function (content, _file, done) {
    const patched = content
      .replace(
        "const loadedAsBrowserEsm =\n    globalThis.document && !globalThis.document.currentScript;",
        "const loadedAsBrowserEsm = false;",
      )
      .replace(
        "const private$ = {};\n    const j$ = {};",
        "const private$ = {};\n    const j$ = { private: private$ };",
      )
      .replace("if (!writeable.includes(k))", "if (false)")
      .replace(
        "this.#global.addEventListener(\n        'rejectionhandled',\n        this.#onRejectionHandled\n      );",
        "",
      )
      .replaceAll("Object.freeze(this);", "");
    done(null, patched);
  };
}
createJasmineZoneCompatPreprocessor.$inject = [];

module.exports = function (config) {
  config.set({
    basePath: "",
    frameworks: ["jasmine"],
    plugins: [
      require("karma-jasmine"),
      require("karma-chrome-launcher"),
      require("karma-jasmine-html-reporter"),
      require("karma-coverage"),
      {
        "preprocessor:jasmine-zone-compat": [
          "factory",
          createJasmineZoneCompatPreprocessor,
        ],
      },
    ],
    preprocessors: {
      [`${require("jasmine-core").files.path}/jasmine.js`]: [
        "jasmine-zone-compat",
      ],
    },
    client: {
      jasmine: {},
      clearContext: false,
    },
    jasmineHtmlReporter: {
      suppressAll: true,
    },
    coverageReporter: {
      dir: require("path").join(__dirname, "./coverage/shipshape-ui"),
      subdir: ".",
      reporters: [{ type: "html" }, { type: "text-summary" }],
      check: {
        global: {
          statements: 100,
          branches: 100,
          functions: 100,
          lines: 100,
        },
      },
    },
    reporters: ["progress", "kjhtml"],
    browsers: ["ChromeHeadless"],
    restartOnFileChange: true,
  });
};
