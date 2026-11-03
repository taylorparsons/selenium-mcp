const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function productionScript() {
  const js = path.join(__dirname, '../src/index_page.js');
  if (fs.existsSync(js)) return fs.readFileSync(js, 'utf8');
  return fs.readFileSync(path.join(__dirname, '../src/index.rs'), 'utf8')
    .match(/const INDEX_SCRIPT: &str = r#"([\s\S]*?)"#;/)[1];
}

function element(tag, attrs = {}, text = '') {
  const el = {
    tagName: tag.toUpperCase(), nodeType: 1, id: attrs.id || '',
    type: attrs.type || '', innerText: text, parentElement: null,
    previousElementSibling: null, childNodes: [],
    get value() { throw new Error('production index must never read form values'); },
    getAttribute(name) { return attrs[name] ?? null; },
    getBoundingClientRect() { return { x: 0, y: 0, width: 100, height: 20 }; },
    style: { display: attrs.hidden !== undefined ? 'none' : 'block', visibility: 'visible' },
  };
  if (text) el.childNodes.push({ nodeType: 3, textContent: text, parentElement: el });
  return el;
}

function index(nodes) {
  const document = {
    body: {}, querySelectorAll: () => nodes,
    createTreeWalker(root, _what, filter) {
      const textNodes = [];
      function walk(el) {
        for (const child of el.childNodes || []) {
          if (child.nodeType === 3) textNodes.push(child); else walk(child);
        }
      }
      walk(root);
      let next = 0;
      return { nextNode() {
        while (next < textNodes.length) {
          const node = textNodes[next++];
          if (!filter || filter.acceptNode(node) === 1) return node;
        }
        return null;
      } };
    },
  };
  for (const el of nodes) if (!el.parentElement) el.parentElement = document.body;
  return vm.runInNewContext(`(function(){${productionScript()}})()`, {
    document, CSS: { escape: s => s },
    NodeFilter: { SHOW_TEXT: 4, FILTER_ACCEPT: 1, FILTER_REJECT: 2 },
    window: { getComputedStyle: el => el.style || { display: 'block', visibility: 'visible' } },
  });
}

test('password, hidden and ordinary input values are never read', () => {
  const rows = index([
    element('input', { id: 'password', type: 'password', 'aria-label': 'Password' }),
    element('input', { id: 'csrf', type: 'hidden' }),
    element('input', { id: 'email', type: 'email', placeholder: 'Email address' }),
    element('textarea', { id: 'notes' }, 'private prefilled notes'),
  ]);
  assert.ok(rows.some(row => row.css === '#password' && row.aria_label === 'Password'));
  assert.ok(rows.some(row => row.css === '#email' && row.placeholder === 'Email address'));
  assert.ok(!rows.some(row => row.css === '#csrf'));
  assert.ok(rows.every(row => row.text === ''));
});

test('hidden and form-control descendants do not leak through a button label', () => {
  const button = element('button', { id: 'submit' }, 'Sign in');
  const hidden = element('span', { hidden: '' }, 'private hidden content');
  const textarea = element('textarea', {}, 'private textarea content');
  hidden.parentElement = button;
  textarea.parentElement = button;
  button.childNodes.push(hidden, textarea);
  button.innerText = 'Sign in private hidden content private textarea content';
  const rows = index([button]);
  assert.equal(rows[0].text, 'Sign in');
});

test('aria-hidden ancestors prevent a descendant from being indexed', () => {
  const parent = element('section', { 'aria-hidden': 'true' });
  const child = element('button', { id: 'private' }, 'private child content');
  child.parentElement = parent;
  assert.equal(index([child]).length, 0);
});

test('inherited editable content is not collected from nested interactive nodes', () => {
  const parent = element('div', { contenteditable: 'true' });
  const child = element('span', { id: 'editable-child', role: 'textbox' }, 'synthetic-sensitive-input');
  child.parentElement = parent;
  parent.childNodes.push(child);
  const rows = index([parent, child]);
  assert.ok(rows.every(row => row.text === ''));
});

test('custom textbox roles are treated as form controls', () => {
  const rows = index([element('span', { id: 'custom-input', role: 'textbox' }, 'synthetic-sensitive-input')]);
  assert.equal(rows[0].text, '');
});
