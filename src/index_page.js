// This script is executed inside the page by WebDriver. Never read input values.
function cssPath(el) {
  if (el.id) return '#' + CSS.escape(el.id);
  var path = [];
  while (el && el.nodeType === 1 && el !== document.body) {
    var selector = el.tagName.toLowerCase();
    if (el.id) { path.unshift('#' + CSS.escape(el.id)); break; }
    var sib = el, nth = 1;
    while (sib.previousElementSibling) {
      sib = sib.previousElementSibling;
      if (sib.tagName === el.tagName) nth++;
    }
    path.unshift(selector + ':nth-of-type(' + nth + ')');
    el = el.parentElement;
  }
  return path.join(' > ');
}

function xpathOf(el) {
  if (el.id && !el.id.includes('"')) return '//*[@id="' + el.id + '"]';
  var parts = [];
  while (el && el.nodeType === 1) {
    var idx = 1, sib = el.previousElementSibling;
    while (sib) { if (sib.tagName === el.tagName) idx++; sib = sib.previousElementSibling; }
    parts.unshift(el.tagName.toLowerCase() + '[' + idx + ']');
    el = el.parentElement;
  }
  return '/' + parts.join('/');
}

function hidden(el) {
  for (var p = el; p && p.nodeType === 1; p = p.parentElement) {
    var style = window.getComputedStyle(p);
    if (p.getAttribute('hidden') !== null || p.getAttribute('aria-hidden') === 'true'
        || style.visibility === 'hidden' || style.visibility === 'collapse'
        || style.display === 'none') return true;
  }
  return false;
}

function formControl(el) {
  return /^(INPUT|TEXTAREA|SELECT|OPTION)$/.test(el.tagName)
    || /^(textbox|searchbox|combobox|spinbutton)$/i.test(el.getAttribute('role') || '')
    || editable(el);
}

function editable(el) {
  for (var p = el; p && p.nodeType === 1; p = p.parentElement) {
    var value = p.getAttribute('contenteditable');
    if (value !== null && value.toLowerCase() !== 'false') return true;
  }
  return false;
}

function safeText(el) {
  if (formControl(el)) return '';
  var walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
    acceptNode: function(node) {
      for (var p = node.parentElement; p && p !== el.parentElement; p = p.parentElement) {
        if (formControl(p) || hidden(p)) return NodeFilter.FILTER_REJECT;
      }
      return NodeFilter.FILTER_ACCEPT;
    }
  });
  var pieces = [], node;
  while ((node = walker.nextNode())) {
    pieces.push(node.textContent);
    if (pieces.join(' ').length > 120) break;
  }
  return pieces.join(' ').replace(/\s+/g, ' ').trim().slice(0, 120);
}

var nodes = document.querySelectorAll('a, button, input, select, textarea, [role], [onclick], [tabindex], [contenteditable]');
var out = [];
for (var i = 0; i < nodes.length; i++) {
  var el = nodes[i], rect = el.getBoundingClientRect();
  var inputType = (el.getAttribute('type') || '').toLowerCase();
  if (inputType === 'hidden' || hidden(el) || !(rect.width || rect.height)) continue;
  out.push({
    tag: el.tagName.toLowerCase(), input_type: inputType, editable: editable(el), role: el.getAttribute('role'),
    text: safeText(el),
    aria_label: el.getAttribute('aria-label'),
    placeholder: el.getAttribute('placeholder'),
    name_or_id: el.getAttribute('name') || el.id || null,
    css: cssPath(el), xpath: xpathOf(el),
    bbox: { x: rect.x, y: rect.y, w: rect.width, h: rect.height },
    visible: true
  });
}
return out;
