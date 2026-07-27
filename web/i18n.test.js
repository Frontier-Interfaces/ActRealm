"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const i18n = require("./i18n.js");

function contract(name) {
  return JSON.parse(fs.readFileSync(path.join(__dirname, "..", "shared", "contracts", name), "utf8"));
}

test("runtime messages cover the shared localized contract", () => {
  for (const message of contract("runtime-messages.json").messages) {
    assert.equal(i18n.RUNTIME_MESSAGES[message.code]?.en, message.en, message.code);
    assert.equal(i18n.RUNTIME_MESSAGES[message.code]?.["zh-Hans"], message["zh-Hans"], message.code);
  }
});

test("API errors cover the shared localized contract", () => {
  for (const error of contract("api-errors.json").errors) {
    assert.equal(i18n.API_ERRORS[error.code]?.en, error.en, error.code);
    assert.equal(i18n.API_ERRORS[error.code]?.["zh-Hans"], error["zh-Hans"], error.code);
  }
});

test("English copy handles singular and plural without changing provider text", () => {
  assert.equal(i18n.tr("1 个任务", "en"), "1 task");
  assert.equal(i18n.tr("3 个任务", "en"), "3 tasks");
  const providerText = "用户项目里的任意 Provider 输出 / do-not-translate";
  assert.equal(i18n.tr(providerText, "en"), providerText);
  assert.equal(i18n.runtimeMessage(undefined, providerText, "en"), providerText);
});

test("bootstrap token is consumed before the first authenticated snapshot", () => {
  assert.equal(i18n.bootstrapPlan({ hashToken: "one-time", csrfToken: "old" }), "bootstrap-first");
  assert.equal(i18n.bootstrapPlan({ csrfToken: "session" }), "snapshot");
  assert.equal(i18n.bootstrapPlan({}), "unauthenticated");
});

test("language preference remains browser-local", () => {
  const saved = new Map();
  global.localStorage = {
    getItem: (key) => saved.get(key) || null,
    setItem: (key, value) => saved.set(key, value),
  };
  global.CustomEvent = class CustomEvent { constructor(type, init) { this.type = type; this.detail = init.detail; } };
  global.dispatchEvent = () => true;
  i18n.setPreference("en");
  assert.equal(saved.get(i18n.STORAGE_KEY), "en");
  delete global.localStorage;
  delete global.CustomEvent;
  delete global.dispatchEvent;
});
