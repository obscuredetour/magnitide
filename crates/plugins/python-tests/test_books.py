"""EPUB files: the metadata reader and writer, the container, the iterator."""

import io
import zipfile

from calibre.ebooks.conversion.preprocess import smarten_punctuation
from calibre.ebooks.metadata.book.base import Metadata
from calibre.ebooks.metadata.epub import get_metadata, set_metadata
from calibre.ebooks.oeb.iterator import EbookIterator
from calibre.ebooks.oeb.polish.container import get_container

OPF = """<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Test Book</dc:title>
    <dc:creator opf:role="aut">Test Author</dc:creator>
    <dc:identifier id="id">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"""
XHTML = '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title></head><body><p>Chapter one has words.</p></body></html>'


def make_epub(path):
    with zipfile.ZipFile(path, 'w') as zf:
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
        zf.writestr(
            'META-INF/container.xml',
            '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
        )
        zf.writestr('OEBPS/content.opf', OPF)
        zf.writestr('OEBPS/ch1.xhtml', XHTML)
        zf.writestr('OEBPS/style.css', 'p { margin: 0 }')
    return str(path)


def test_epub_metadata_round_trip(tmp_path):
    path = make_epub(tmp_path / 'b.epub')
    with open(path, 'rb') as f:
        mi = get_metadata(f)
    assert (mi.title, mi.authors, mi.languages) == ('Test Book', ['Test Author'], ['eng'])
    new = Metadata('Changed', ['Someone Else'])
    new.tags = ['x']
    with open(path, 'r+b') as f:
        set_metadata(f, new)
    with open(path, 'rb') as f:
        mi = get_metadata(f)
    assert (mi.title, mi.authors, mi.tags) == ('Changed', ['Someone Else'], ['x'])
    with zipfile.ZipFile(path) as zf:
        assert zf.namelist()[0] == 'mimetype'


def test_container(tmp_path):
    path = make_epub(tmp_path / 'c.epub')
    c = get_container(path)
    assert c.opf_name == 'OEBPS/content.opf' and c.opf_version_parsed.major == 2
    assert list(c.spine_names) == [('OEBPS/ch1.xhtml', True)]
    assert c.mime_map['OEBPS/style.css'] == 'text/css'
    root = c.parsed('OEBPS/ch1.xhtml')
    assert root.tag.endswith('}html')
    body = root.find('{http://www.w3.org/1999/xhtml}body')
    body[0].text = 'Changed text.'
    c.dirty('OEBPS/ch1.xhtml')
    c.add_file('OEBPS/ch2.xhtml', XHTML.replace('one', 'two').encode(), spine_index=1)
    assert [n for n, _ in c.spine_names] == ['OEBPS/ch1.xhtml', 'OEBPS/ch2.xhtml']
    c.remove_item('OEBPS/style.css')
    out = str(tmp_path / 'out.epub')
    c.commit(out)
    again = get_container(out)
    assert 'OEBPS/style.css' not in again.name_path_map
    assert 'Changed text.' in again.raw_data('OEBPS/ch1.xhtml')
    assert [n for n, _ in again.spine_names] == ['OEBPS/ch1.xhtml', 'OEBPS/ch2.xhtml']


def test_iterator(tmp_path):
    path = make_epub(tmp_path / 'i.epub')
    it = EbookIterator(path)
    it.__enter__(only_input_plugin=True, run_char_count=True, read_anchor_map=False)
    try:
        assert len(it.spine) == 1
        assert it.spine[0].character_count > 10
        assert it.opf.language == 'eng' and it.mi.title == 'Test Book'
    finally:
        it.__exit__()


def test_smarten_punctuation():
    out = smarten_punctuation('"Hi" -- it\'s <pre>"raw"</pre>...')
    assert out == '“Hi” – it’s <pre>"raw"</pre>…'


def test_images(qapp):
    from calibre.utils.img import image_to_data, scale_image
    from calibre.utils.magick import create_canvas

    canvas = create_canvas(40, 60, '#336699')
    png = canvas.export('png')
    w, h, data = scale_image(png, 20, 20)
    assert (w, h) == (13, 20) and data[:3] == b'\xff\xd8\xff'
    assert image_to_data(io.BytesIO(png).getvalue() and canvas.img, fmt='PNG')[:4] == b'\x89PNG'
