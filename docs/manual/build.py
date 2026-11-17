"""Builds the OOS Plus SCPI Programming Manual (PDF).

    python docs/manual/build.py [out.pdf]

Inputs beside this script: overview.html (chapters written by hand), commands.json (the command reference and the
item tables), style.css. Needs Node and Chrome (print.cjs) and PyMuPDF (page numbers in the contents, bookmarks).
"""
import html
import json
import os
import re
import subprocess
import sys
import tempfile

import pymupdf as fitz

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(HERE, '..', 'OOS-Plus-SCPI-Programming-Manual.pdf')


def inline(s):
    """Escaped text with `code` and **bold**."""
    s = html.escape(s, quote=False)
    s = re.sub(r'`([^`]+)`', r'<code>\1</code>', s)
    return re.sub(r'\*\*([^*]+)\*\*', r'<b>\1</b>', s)


def para(s):
    """One or more paragraphs; a list item starts with '- '."""
    if isinstance(s, list):
        items = ''.join(f'<li>{inline(x[2:] if x.startswith("- ") else x)}</li>' for x in s)
        return f'<ul>{items}</ul>'
    return ''.join(f'<p>{inline(p)}</p>' for p in s.split('\n\n'))


def slug(s):
    return re.sub(r'[^a-z0-9]+', '-', s.lower()).strip('-')


def command(c, sid):
    cid = 'cmd-' + slug(c['header'])
    ext = '<span class="ext">OOS</span>' if c.get('ext') else ''
    rows = []
    syn = [c[k] for k in ('set', 'query') if c.get(k)]
    rows.append(('Syntax', ''.join(f'<span class="syntax">{html.escape(x)}</span>' for x in syn)))
    if c.get('aliases'):
        rows.append(('Aliases', ', '.join(f'<code>{html.escape(a)}</code>' for a in c['aliases'])))
    rows.append(('Description', para(c['desc'])))
    if c.get('params'):
        body = ''.join('<tr>' + ''.join(f'<td>{inline(str(v)) if v not in (None, "") else "—"}</td>' for v in p) + '</tr>' for p in c['params'])
        rows.append(('Parameter', f'<table><thead><tr><th>Name</th><th>Type</th><th>Range</th><th>Default</th></tr></thead><tbody>{body}</tbody></table>'))
    if c.get('remarks'):
        rows.append(('Remarks', para(c['remarks'])))
    if c.get('returns'):
        rows.append(('Return format', para(c['returns'])))
    if c.get('example'):
        rows.append(('Example', f'<pre>{html.escape(c["example"])}</pre>'))
    dl = ''.join(f'<dt>{k}</dt><dd>{v}</dd>' for k, v in rows)
    long = ' long' if len(c.get('params') or []) > 6 or len(c.get('example', '')) > 600 else ''
    return f'<div class="cmd{long}"><h3 id="{cid}">{html.escape(c["header"])}{ext}</h3><dl>{dl}</dl></div>'


def table(t):
    head = ''.join(f'<th>{inline(h)}</th>' for h in t['columns'])
    body = ''.join('<tr>' + ''.join(f'<td>{inline(str(v))}</td>' for v in r) + '</tr>' for r in t['rows'])
    intro = para(t['intro']) if t.get('intro') else ''
    return f'{intro}<table class="long"><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>'


def reference(data, chapter):
    out = [f'<h1 id="ch-commands"><span class="num">{chapter}</span>Command System</h1>', para(data['intro'])]
    for i, s in enumerate(data['subsystems'], 1):
        sid = 'sub-' + slug(s['title'])
        out.append(f'<h2 id="{sid}"><span class="num">{chapter}.{i}</span>{html.escape(s["title"])}</h2>')
        if s.get('intro'):
            out.append(para(s['intro']))
        out.extend(command(c, sid) for c in s['commands'])
    return '\n'.join(out)


def appendices(data, first):
    out = []
    for i, a in enumerate(data['appendices']):
        letter = chr(ord(first) + i)
        out.append(f'<h1 id="app-{slug(a["title"])}"><span class="num">Appendix {letter}</span>{html.escape(a["title"])}</h1>')
        if a.get('intro'):
            out.append(para(a['intro']))
        for t in a['tables']:
            if t.get('title'):
                out.append(f'<h2 id="app-{slug(a["title"])}-{slug(t["title"])}">{html.escape(t["title"])}</h2>')
            out.append(table(t))
    return '\n'.join(out)


HEAD = re.compile(r'<h([123]) id="([^"]+)"[^>]*>(.*?)</h\1>', re.S)


def contents(body, pages):
    items = []
    for level, hid, text in HEAD.findall(body):
        label = re.sub(r'<span class="ext">.*?</span>', '', text)
        label = re.sub(r'<span class="num">(.*?)</span>', r'\1  ', label)
        label = re.sub(r'<[^>]+>', '', label)
        p = pages.get(hid, '')
        items.append(f'<li class="l{level}"><a class="t" href="#{hid}">{label}</a><span class="dots"></span><a class="p" href="#{hid}">{p}</a></li>')
    return f'<section class="toc"><h1 id="toc">Contents</h1><ol>{"".join(items)}</ol></section>'


def document(pages):
    data = json.load(open(os.path.join(HERE, 'commands.json'), encoding='utf-8'))
    overview = open(os.path.join(HERE, 'overview.html'), encoding='utf-8').read()
    before, after = overview.split('<!-- COMMANDS -->')
    meta = data['meta']
    body = before + reference(data, meta['commands_chapter']) + after + appendices(data, 'A')
    cover = f'''<section class="cover"><div class="bar"></div><div class="kicker">Programming Manual</div>
<h1>OOS Plus<br>SCPI Programming Manual</h1><div class="sub">Remote control of OOS Plus oscilloscopes and the OOS desktop app</div>
<div class="meta">Software: {html.escape(meta["software"])}<br>Edition: {html.escape(meta["edition"])}<br>Instrument dialect: {html.escape(meta["dialect"])}</div></section>'''
    css = open(os.path.join(HERE, 'style.css'), encoding='utf-8').read()
    return (f'<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>OOS Plus SCPI Programming Manual</title>'
            f'<style>{css}</style></head><body>{cover}{contents(body, pages)}{body}</body></html>'), body


def render(doc, pdf):
    with tempfile.NamedTemporaryFile('w', suffix='.html', delete=False, encoding='utf-8') as f:
        f.write(doc)
    try:
        subprocess.run(['node', os.path.join(HERE, 'print.cjs'), f.name, pdf], check=True)
    finally:
        os.unlink(f.name)


def anchor_pages(pdf, ids):
    """Page (1-based) of each heading, from the contents links Chrome writes as in-document jumps."""
    d = fitz.open(pdf)
    wanted, found = set(ids), {}
    for page in d:
        for link in page.get_links():
            name = link.get('nameddest')
            if name in wanted and link.get('page', -1) >= 0:
                found[name] = link['page'] + 1
    d.close()
    return found


def main():
    _, body = document({})
    ids = [hid for _, hid, _ in HEAD.findall(body)]
    tmp = OUT + '.pass1.pdf'
    doc, _ = document({i: '000' for i in ids})
    render(doc, tmp)
    pages = anchor_pages(tmp, ids)
    os.unlink(tmp)
    missing = [i for i in ids if i not in pages]
    if missing:
        sys.exit(f'no page found for {len(missing)} headings, e.g. {missing[:5]}')
    doc, _ = document(pages)
    render(doc, OUT)

    d = fitz.open(OUT)
    labels = {hid: (int(lv), re.sub(r'<[^>]+>', '', re.sub(r'<span class="num">(.*?)</span>', r'\1  ', re.sub(r'<span class="ext">.*?</span>', '', t))).strip()) for lv, hid, t in HEAD.findall(body)}
    toc = [[1, 'Contents', 2]] + [[labels[h][0], labels[h][1], pages[h]] for h in ids]
    d.set_toc(toc)
    cover = d[0]  # no footer on the cover
    cover.draw_rect(fitz.Rect(0, cover.rect.height - 40, cover.rect.width, cover.rect.height), color=None, fill=(1, 1, 1))
    cover.draw_rect(fitz.Rect(0, cover.rect.height - 40, 9 / 25.4 * 72, cover.rect.height), color=None, fill=(0x1f / 255, 0x3b / 255, 0x5a / 255))
    d.set_metadata({'title': 'OOS Plus SCPI Programming Manual', 'subject': 'SCPI remote control', 'creator': 'docs/manual/build.py'})
    d.saveIncr()
    print(f'{OUT}: {len(d)} pages, {len(ids)} headings')
    d.close()


if __name__ == '__main__':
    main()
