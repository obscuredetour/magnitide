"""A metadata writer for Magnitide's plugin host tests: rewrites the
"Field: value" header of a TXT file in place."""

from calibre.customize import MetadataWriterPlugin


class TestWriter(MetadataWriterPlugin):
    name = 'Magnitide Test Writer'
    description = 'Writes a "Field: value" header into TXT files'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    file_types = {'txt'}
    priority = 2

    def set_metadata(self, stream, mi, type):
        text = stream.read().decode('utf-8', 'replace')
        body = []
        in_header = True
        for line in text.splitlines():
            if in_header and (': ' in line or not line.strip()):
                continue
            in_header = False
            body.append(line)
        header = [f'Title: {mi.title}', f'Author: {" & ".join(mi.authors or [])}']
        isbn = mi.get_identifiers().get('isbn')
        if isbn:
            header.append(f'ISBN: {isbn}')
        if mi.tags:
            header.append(f'Tags: {", ".join(mi.tags)}')
        stream.seek(0)
        stream.truncate()
        stream.write('\n'.join(header + [''] + body).encode('utf-8'))
