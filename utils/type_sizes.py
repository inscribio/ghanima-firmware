#!/usr/bin/env python

import re
import os
import pprint
import logging
import argparse
import subprocess
import dataclasses
from pathlib import Path
from typing import Union, Optional

import jinja2


log = logging.getLogger('type_sizes')
logging.basicConfig(level=logging.WARN, format='[%(levelname)s] %(message)s')


def touch(path):
    path = Path(path)
    assert path.is_file(), f'Refusing to touch non-existing file: "{path}"'
    path.touch()


def compile(args):
    cmd = ['cargo', '+nightly', 'rustc', *args, '--', '-Zprint-type-sizes']
    proc = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return proc


def g(name, pattern):
    return r'(?P<{}>{})'.format(name, pattern)


def regex(fmt, **groups):
    kwargs = {
        name: g(name, pattern)
        for (name, pattern) in groups.items()
    }
    pattern = fmt.format(**kwargs)
    return re.compile(pattern)

INDENT = 4

PATTERNS = {
    'main': regex(r'^print-type-size {tail}$', tail=r'.*'),
    'type': regex(r'^type: `{name}`: {size} bytes, alignment: {align} bytes$',
                  name=r'[^`]+', size=r'\d+', align=r'\d+'),
    'inner': regex(r'^{indent}{type}( `{name}`)?: {size} bytes(, offset: {offset} bytes)?(, alignment: {align} bytes)?$',
                   indent=r'\s+', type=r'[a-z ]+', name=r'[^`]+', size=r'\d+', offset=r'\d+', align=r'\d+'),
}


@dataclasses.dataclass()
class Discriminant:
    size: int

    def to_html(self):
        return f'<li>Discriminant: {self.size} bytes</li>'


@dataclasses.dataclass()
class Padding:
    size: int

    def to_html(self):
        return f'<li>Padding: {self.size} bytes</li>'


@dataclasses.dataclass()
class EndPadding:
    size: int

    def to_html(self):
        return f'<li>End padding: {self.size} bytes</li>'


@dataclasses.dataclass()
class Field:
    name: str
    size: int
    offset: Optional[int]
    alignment: Optional[int]

    def to_html(self):
        html = f'Field {self.name}: {self.size} bytes'
        if self.offset:
            html += f', offset: {self.offset} bytes'
        if self.alignment:
            html += f', alignment: {self.alignment} bytes'
        return f'<li>{html}</li>'


@dataclasses.dataclass()
class Variant:
    name: str
    size: int
    tree: list[Union[Padding, Field]]

    def to_html(self):
        simple = f'Variant {self.name}: {self.size} bytes'
        if self.tree is None:
            return f'<li>{simple}</li>'

        nested = [node.to_html() for node in self.tree]
        return '''
<li>
    <span class="caret">{name}</span>
    <ul class="nested">
{nested}
    </ul>
</li>
        '''.strip().format(name=simple, nested='\n'.join(nested))


@dataclasses.dataclass()
class Type:
    name: str
    size: int
    alignment: int
    tree: list[Union[Discriminant, Padding, EndPadding, Variant, Field]]

    def to_html(self):
        simple = f'Type {self.name}: {self.size} bytes, alignment {self.alignment} bytes'
        if not self.tree:
            return f'<li>{simple}</li>'

        nested = [node.to_html() for node in self.tree]
        return '''
<li>
    <span class="caret">{name}</span>
    <ul class="nested">
{nested}
    </ul>
</li>
        '''.strip().format(name=simple, nested='\n'.join(nested))


def parse(lines) -> list[Type]:
    # for name, pattern in PATTERNS.items():
    #     print(f'PATTERN {name}:')
    #     print(pattern.pattern)
    # print()

    # filter out non-related lines
    lines = map(PATTERNS['main'].match, lines)
    lines = filter(None, lines)
    lines = map(lambda m: m.group('tail'), lines)

    lines = list(lines)

    types = []
    while len(lines):
        lines, typ = parse_type(lines)
        if typ is not None:
            types.append(typ)

    return types


def parse_type(lines) -> tuple[list[str], Type]:
    line, *lines = lines

    match = PATTERNS['type'].match(line)
    if not match:
        log.error('Ignoring line (expected type): %s', line)
        return lines

    lines, tree = parse_tree(lines)

    return lines, Type(
        name=match.group('name'),
        size=int(match.group('size')),
        alignment=int(match.group('align')),
        tree=tree,
    )


def parse_tree(lines, depth=1) -> tuple[list[str], list[Union[Discriminant, Padding, Variant, Field]]]:
    tree = []

    while len(lines) > 0:
        # Iterate until we find "type" line again
        match = PATTERNS['inner'].match(lines[0])
        if not match:
            return lines, tree

        group = lambda name: match.group(name)
        igroup = lambda name: int(group(name))
        igroup_opt = lambda name: int(group(name)) if group(name) else None

        indent = len(group('indent'))

        # Test line indent
        if indent > depth * INDENT:  # Parse inner subtree
            lines, subtree = parse_tree(lines, depth=depth + 1)

            prev = tree[-1]
            assert isinstance(prev, Variant)
            prev.tree = subtree

            continue
        elif indent < depth * INDENT:  # Go up the tree
            return lines, tree

        # consume this line
        line, *lines = lines

        typ = group('type')
        result = None
        if typ == 'discriminant':
            result = Discriminant(size=group('size'))
        elif typ == 'padding':
            result = Padding(size=igroup('size'))
        elif typ == 'end padding':
            result = EndPadding(size=igroup('size'))
        elif typ == 'field':
            result = Field(
                name=group('name'),
                size=igroup('size'),
                offset=igroup_opt('offset'),
                alignment=igroup_opt('align'),
            )
        elif typ == 'variant':
            result = Variant(name=group('name'), size=igroup('size'), tree=None)

        assert result is not None, f'Parsing failed on: {line}'
        tree.append(result)

    return lines, tree


def main(args=None):
    description = '''
    Show type sizes in Rust code. Compiles the code using
    `cargo +nightly rustc <args> -- -Zprint-type-sizes`
    and parses the compiler output to obtain sizes of types.
    All unlisted arguments are passed as `<args>` to `cargo rustc`.
    '''
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument('--touch', default='src/main.rs',
                        help='Touch this file to force re-linking')
    parser.add_argument('--output', default='html', choices=['html', 'pprint'],
                        help='Output type')
    parser.add_argument('--sort-size', action='store_true', help='Sort by size')
    args, tail = parser.parse_known_args()

    touch(args.touch)
    proc = compile(tail)
    types = parse(proc.stdout.split('\n'))

    if args.sort_size:
        types = sorted(types, key=lambda typ: typ.size)

    if args.output == 'pprint':
        for typ in types:
            pprint.pprint(typ)
    elif args.output == 'html':
        this_dir = os.path.dirname(os.path.abspath(os.path.realpath(__file__)))
        templates_dir = os.path.join(this_dir, 'type_sizes', 'templates')
        output_dir = os.path.join(this_dir, 'type_sizes')
        input = 'index.jinja2.html'
        output = 'index.html'

        env = jinja2.Environment(loader=jinja2.FileSystemLoader(templates_dir), trim_blocks=True)
        template = env.get_template(input)
        template.stream(types=types).dump(os.path.join(output_dir, output))

        print('HTML output saved to:', os.path.join(output_dir, output))
    else:
        raise ValueError(args.output)

if __name__ == "__main__":
    main()
