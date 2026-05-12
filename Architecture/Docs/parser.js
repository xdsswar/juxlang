// Jux docs — minimal but capable Markdown → HTML renderer
// Handles: headings, paragraphs, fenced code blocks, inline code,
// bold/italic, links, tables, blockquotes, lists, horizontal rules,
// auto-anchors on headings, light syntax highlighting for jux/rust.

(function (global) {
  'use strict';

  function escapeHtml(s) {
    return s
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function slug(s) {
    return s
      .toLowerCase()
      .replace(/<[^>]+>/g, '')
      .replace(/[^\w\s\-§\.]/g, '')
      .trim()
      .replace(/\s+/g, '-')
      .substring(0, 80);
  }

  // Light syntax highlighting for code blocks
  var keywords = new Set([
    'abstract', 'async', 'await', 'break', 'case', 'catch', 'class', 'const',
    'default', 'do', 'drop', 'else', 'enum', 'extends', 'final', 'finally',
    'for', 'if', 'implements', 'import', 'init', 'interface', 'internal',
    'move', 'native', 'new', 'open', 'package', 'permits', 'private',
    'protected', 'public', 'record', 'return', 'sealed', 'static', 'struct',
    'super', 'switch', 'this', 'throw', 'throws', 'try', 'type', 'unsafe',
    'var', 'void', 'volatile', 'when', 'while', 'yield', 'operator',
    'true', 'false', 'null', 'where', 'has',
    // also rust keywords for rust code blocks
    'fn', 'let', 'mut', 'pub', 'use', 'impl', 'trait', 'mod', 'crate',
    'self', 'Self', 'where', 'as', 'dyn', 'box', 'ref'
  ]);

  function highlight(code, lang) {
    if (!lang || (lang !== 'jux' && lang !== 'java' && lang !== 'rust' &&
                  lang !== 'js' && lang !== 'javascript' && lang !== 'c' &&
                  lang !== 'cpp')) {
      return escapeHtml(code);
    }
    // Process character-by-character
    var out = '';
    var i = 0;
    var n = code.length;
    while (i < n) {
      var c = code[i];
      // Line comment
      if (c === '/' && code[i+1] === '/') {
        var end = code.indexOf('\n', i);
        if (end === -1) end = n;
        out += '<span class="com">' + escapeHtml(code.substring(i, end)) + '</span>';
        i = end;
        continue;
      }
      // Block comment
      if (c === '/' && code[i+1] === '*') {
        var endb = code.indexOf('*/', i);
        if (endb === -1) endb = n; else endb += 2;
        out += '<span class="com">' + escapeHtml(code.substring(i, endb)) + '</span>';
        i = endb;
        continue;
      }
      // String
      if (c === '"' || c === "'") {
        var quote = c;
        var j = i + 1;
        while (j < n && code[j] !== quote) {
          if (code[j] === '\\') j++;
          j++;
        }
        j++;
        out += '<span class="str">' + escapeHtml(code.substring(i, j)) + '</span>';
        i = j;
        continue;
      }
      // Number
      if (/[0-9]/.test(c) && (i === 0 || !/[A-Za-z_]/.test(code[i-1]))) {
        var k = i;
        while (k < n && /[0-9_xXa-fA-FoObB.eELlfFuUsS]/.test(code[k])) k++;
        out += '<span class="num">' + escapeHtml(code.substring(i, k)) + '</span>';
        i = k;
        continue;
      }
      // Identifier / keyword
      if (/[A-Za-z_]/.test(c)) {
        var m = i;
        while (m < n && /[A-Za-z0-9_]/.test(code[m])) m++;
        var word = code.substring(i, m);
        if (keywords.has(word)) {
          out += '<span class="kw">' + word + '</span>';
        } else if (/^[A-Z]/.test(word)) {
          out += '<span class="typ">' + word + '</span>';
        } else {
          out += escapeHtml(word);
        }
        i = m;
        continue;
      }
      out += escapeHtml(c);
      i++;
    }
    return out;
  }

  function renderInline(text) {
    // Escape HTML first; we'll then re-introduce specific markup
    var s = escapeHtml(text);

    // Inline code: `code`
    s = s.replace(/`([^`]+)`/g, function(_, c) {
      return '<code>' + c + '</code>';
    });

    // Links: [text](url)
    s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, function(_, t, u) {
      var url = u.replace(/&amp;/g, '&');
      return '<a href="' + url + '">' + t + '</a>';
    });

    // Bold: **text**
    s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

    // Italic: *text*  (only if not already inside strong; simple approach)
    s = s.replace(/(^|\W)\*([^\s*][^*]*[^\s*]|\S)\*(?=\W|$)/g, '$1<em>$2</em>');

    return s;
  }

  function isTableSeparator(line) {
    return /^\s*\|?\s*:?-+:?(\s*\|\s*:?-+:?)+\s*\|?\s*$/.test(line);
  }

  function parseTableRow(line) {
    var trimmed = line.trim();
    if (trimmed.startsWith('|')) trimmed = trimmed.substring(1);
    if (trimmed.endsWith('|')) trimmed = trimmed.substring(0, trimmed.length - 1);
    return trimmed.split('|').map(function(c) { return c.trim(); });
  }

  function render(md) {
    // Normalize line endings
    md = md.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
    var lines = md.split('\n');
    var out = [];
    var i = 0;
    var n = lines.length;

    while (i < n) {
      var line = lines[i];

      // Fenced code block
      var fenceMatch = line.match(/^```(\S*)\s*$/);
      if (fenceMatch) {
        var lang = fenceMatch[1] || '';
        var code = [];
        i++;
        while (i < n && !/^```\s*$/.test(lines[i])) {
          code.push(lines[i]);
          i++;
        }
        i++; // consume closing fence
        out.push('<pre><code class="lang-' + escapeHtml(lang) + '">' +
                 highlight(code.join('\n'), lang) + '</code></pre>');
        continue;
      }

      // Heading
      var headingMatch = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/);
      if (headingMatch) {
        var level = headingMatch[1].length;
        var text = headingMatch[2];
        var rendered = renderInline(text);
        var id = slug(text);
        out.push('<h' + level + ' id="' + id + '">' + rendered + '</h' + level + '>');
        i++;
        continue;
      }

      // Horizontal rule
      if (/^(\s*)(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
        out.push('<hr>');
        i++;
        continue;
      }

      // Table
      if (line.includes('|') && i + 1 < n && isTableSeparator(lines[i + 1])) {
        var headers = parseTableRow(line);
        i += 2; // skip header + separator
        var rows = [];
        while (i < n && lines[i].trim() !== '' && lines[i].includes('|')) {
          rows.push(parseTableRow(lines[i]));
          i++;
        }
        var t = '<table>\n<thead><tr>';
        for (var h = 0; h < headers.length; h++) {
          t += '<th>' + renderInline(headers[h]) + '</th>';
        }
        t += '</tr></thead>\n<tbody>';
        for (var r = 0; r < rows.length; r++) {
          t += '<tr>';
          for (var c = 0; c < rows[r].length; c++) {
            t += '<td>' + renderInline(rows[r][c]) + '</td>';
          }
          t += '</tr>';
        }
        t += '</tbody></table>';
        out.push(t);
        continue;
      }

      // Blockquote
      if (/^\s*>\s?/.test(line)) {
        var bq = [];
        while (i < n && /^\s*>\s?/.test(lines[i])) {
          bq.push(lines[i].replace(/^\s*>\s?/, ''));
          i++;
        }
        out.push('<blockquote>' + render(bq.join('\n')) + '</blockquote>');
        continue;
      }

      // Unordered list
      if (/^\s*[-*+]\s+/.test(line)) {
        var ulItems = [];
        var baseIndent = (line.match(/^(\s*)/) || ['', ''])[1].length;
        while (i < n) {
          var li = lines[i];
          var liMatch = li.match(/^(\s*)[-*+]\s+(.*)$/);
          if (!liMatch) {
            // Continuation: indented or blank-then-indented?
            if (/^\s*$/.test(li)) break;
            // Lazy continuation (regular text after list item)
            if (ulItems.length > 0 && /^\s+\S/.test(li)) {
              ulItems[ulItems.length - 1] += '\n' + li.trim();
              i++;
              continue;
            }
            break;
          }
          if (liMatch[1].length < baseIndent) break;
          ulItems.push(liMatch[2]);
          i++;
        }
        var ulHtml = '<ul>';
        for (var u = 0; u < ulItems.length; u++) {
          ulHtml += '<li>' + renderInline(ulItems[u]) + '</li>';
        }
        ulHtml += '</ul>';
        out.push(ulHtml);
        continue;
      }

      // Ordered list
      if (/^\s*\d+\.\s+/.test(line)) {
        var olItems = [];
        var olBase = (line.match(/^(\s*)/) || ['', ''])[1].length;
        while (i < n) {
          var oli = lines[i];
          var oliMatch = oli.match(/^(\s*)\d+\.\s+(.*)$/);
          if (!oliMatch) {
            if (/^\s*$/.test(oli)) break;
            if (olItems.length > 0 && /^\s+\S/.test(oli)) {
              olItems[olItems.length - 1] += '\n' + oli.trim();
              i++;
              continue;
            }
            break;
          }
          if (oliMatch[1].length < olBase) break;
          olItems.push(oliMatch[2]);
          i++;
        }
        var olHtml = '<ol>';
        for (var o = 0; o < olItems.length; o++) {
          olHtml += '<li>' + renderInline(olItems[o]) + '</li>';
        }
        olHtml += '</ol>';
        out.push(olHtml);
        continue;
      }

      // Blank line
      if (/^\s*$/.test(line)) {
        i++;
        continue;
      }

      // Paragraph
      var para = [line];
      i++;
      while (i < n && lines[i].trim() !== '' &&
             !/^(#{1,6})\s/.test(lines[i]) &&
             !/^```/.test(lines[i]) &&
             !/^\s*[-*+]\s+/.test(lines[i]) &&
             !/^\s*\d+\.\s+/.test(lines[i]) &&
             !/^\s*>\s?/.test(lines[i]) &&
             !/^(\s*)(-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i])) {
        // Stop also if next is a table
        if (lines[i].includes('|') && i + 1 < n && isTableSeparator(lines[i + 1])) break;
        para.push(lines[i]);
        i++;
      }
      out.push('<p>' + renderInline(para.join('\n')) + '</p>');
    }

    return out.join('\n');
  }

  function buildToc(html) {
    // Extract h2 and h3 headings into a list of TOC entries
    var div = document.createElement('div');
    div.innerHTML = html;
    var hs = div.querySelectorAll('h2, h3');
    var entries = [];
    for (var i = 0; i < hs.length; i++) {
      entries.push({
        level: parseInt(hs[i].tagName.substring(1), 10),
        text: hs[i].textContent,
        id: hs[i].id
      });
    }
    return entries;
  }

  function init() {
    var src = document.getElementById('md-content');
    if (!src) return;
    var md = src.textContent;
    var html = render(md);
    var target = document.getElementById('rendered');
    if (target) {
      target.innerHTML = html;
    }
    // Build TOC if requested
    var tocTarget = document.getElementById('toc');
    if (tocTarget) {
      var entries = buildToc(html);
      var ul = '<ul class="toc-list">';
      for (var k = 0; k < entries.length; k++) {
        var e = entries[k];
        ul += '<li class="toc-l' + e.level + '"><a href="#' + e.id + '">' +
              e.text + '</a></li>';
      }
      ul += '</ul>';
      tocTarget.innerHTML = ul;
    }
    // Highlight active sidebar item
    var path = (location.pathname.split('/').pop() || '').toLowerCase();
    var sideLinks = document.querySelectorAll('.sidebar a');
    for (var s = 0; s < sideLinks.length; s++) {
      var href = (sideLinks[s].getAttribute('href') || '').toLowerCase();
      if (href === path) sideLinks[s].classList.add('active');
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  global.JuxDocs = { render: render };
})(window);
