"""Mirrors calibre's `calibre.utils.zipfile` (calibre 9.15): the standard
library's zipfile with calibre's additions (`safe_replace`, `delete`,
`add_dir`, safe `extractall`, `ZipFile.replace`)."""

import os
import shutil
import stat
import zipfile as _zf
from zipfile import (  # noqa: F401
    ZIP_BZIP2,
    ZIP_DEFLATED,
    ZIP_LZMA,
    ZIP_STORED,
    BadZipFile,
    BadZipfile,
    LargeZipFile,
    ZipExtFile,
    ZipInfo,
    is_zipfile,
)

from calibre.ptempfile import SpooledTemporaryFile


def safe_replace(zipstream, name, datastream, extra_replacements={}, add_missing=False):
    """Replace `name` in the zip at `zipstream` (a file object opened r+b)
    with `datastream`'s contents, and likewise each of `extra_replacements`
    (name → stream). Entries not in the zip are added when `add_missing`."""
    replacements = {name: datastream}
    replacements.update(extra_replacements)
    names = frozenset(replacements)
    found = set()

    def rbytes(name):
        r = replacements[name]
        if not isinstance(r, bytes):
            r = r.read()
        return r

    with SpooledTemporaryFile(max_size=100 * 1024 * 1024) as temp:
        ztemp = ZipFile(temp, 'w')
        with ZipFile(zipstream, 'r') as zin:
            for obj in zin.infolist():
                if obj.filename in names:
                    ztemp.writestr(obj, rbytes(obj.filename))
                    found.add(obj.filename)
                else:
                    ztemp.writestr(obj, zin.read_raw(obj), raw_bytes=True)
        if add_missing:
            for name in names - found:
                ztemp.writestr(name, rbytes(name))
        ztemp.close()
        zipstream.seek(0)
        zipstream.truncate()
        temp.seek(0)
        shutil.copyfileobj(temp, zipstream)
        zipstream.flush()


class ZipFile(_zf.ZipFile):
    def read_raw(self, name_or_info):
        """An entry's compressed bytes, for copying it to another zip as is."""
        if not isinstance(name_or_info, ZipInfo):
            name_or_info = self.getinfo(name_or_info)
        with self.open(name_or_info):
            # The stdlib decompresses; re-reading raw needs the file position.
            pass
        fp = self.fp
        info = name_or_info
        fp.seek(info.header_offset)
        fheader = fp.read(_zf.sizeFileHeader)
        fheader = _zf.struct.unpack(_zf.structFileHeader, fheader)
        fp.read(fheader[_zf._FH_FILENAME_LENGTH] + fheader[_zf._FH_EXTRA_FIELD_LENGTH])
        return fp.read(info.compress_size)

    def writestr(self, zinfo_or_arcname, data, compress_type=None, permissions=0o600, raw_bytes=False):
        if raw_bytes:
            info = zinfo_or_arcname
            if not isinstance(info, ZipInfo):
                raise TypeError('raw_bytes needs a ZipInfo')
            # Write the entry's header and the compressed bytes untouched.
            self._writecheck(info)
            info.header_offset = self.fp.tell()
            self.fp.write(info.FileHeader(False))
            self.fp.write(data)
            self.filelist.append(info)
            self.NameToInfo[info.filename] = info
            self.start_dir = self.fp.tell()
            return
        if isinstance(data, str):
            data = data.encode('utf-8')
        if not isinstance(zinfo_or_arcname, ZipInfo):
            zinfo = ZipInfo(filename=zinfo_or_arcname, date_time=_zf.time.localtime(_zf.time.time())[:6])
            zinfo.compress_type = self.compression if compress_type is None else compress_type
            zinfo.external_attr = (stat.S_IFREG | permissions) << 16
            return _zf.ZipFile.writestr(self, zinfo, data)
        return _zf.ZipFile.writestr(self, zinfo_or_arcname, data, compress_type=compress_type)

    def delete(self, name):
        """Remove an entry (by rewriting the zip)."""
        if self.mode != 'a':
            raise RuntimeError('delete needs mode "a"')
        with SpooledTemporaryFile(max_size=100 * 1024 * 1024) as temp:
            with ZipFile(temp, 'w') as ztemp:
                for obj in self.infolist():
                    if obj.filename != name:
                        ztemp.writestr(obj, self.read_raw(obj), raw_bytes=True)
            temp.seek(0)
            self.fp.seek(0)
            self.fp.truncate()
            shutil.copyfileobj(temp, self.fp)
        self.fp.seek(0)
        self._RealGetContents()

    def replace(self, filename, arcname=None, compress_type=None):
        """Replace an entry with the file at `filename`."""
        if arcname is None:
            arcname = os.path.basename(filename)
        with open(filename, 'rb') as f:
            data = f.read()
        self.replacestr(arcname, data, compress_type=compress_type)

    def replacestr(self, zinfo_or_arcname, data, compress_type=None):
        name = zinfo_or_arcname.filename if isinstance(zinfo_or_arcname, ZipInfo) else zinfo_or_arcname
        if name in self.NameToInfo:
            self.delete(name)
        self.writestr(zinfo_or_arcname, data, compress_type=compress_type)

    def add_dir(self, path, prefix='', simple_filter=lambda x: False):
        """Add a folder's contents, recursively, under `prefix`."""
        for root, dirs, files in os.walk(path):
            for f in files:
                p = os.path.join(root, f)
                arc = os.path.join(prefix, os.path.relpath(p, path)).replace(os.sep, '/')
                if simple_filter(arc):
                    continue
                self.write(p, arc)

    def extractall(self, path=None, members=None, pwd=None):
        """Extract, refusing entries that would land outside `path`."""
        if path is None:
            path = os.getcwd()
        path = os.path.abspath(path)
        for member in members if members is not None else self.namelist():
            name = member.filename if isinstance(member, ZipInfo) else member
            target = os.path.abspath(os.path.join(path, name))
            if not target.startswith(path + os.sep) and target != path:
                raise ValueError(f'Zip entry outside the destination: {name}')
        return _zf.ZipFile.extractall(self, path, members, pwd)

    def extract_to_dir(self, path):
        self.extractall(path)

    def safe_replace(self, name, datastream, extra_replacements={}, add_missing=False):
        self.close()
        with open(self.filename, 'r+b') as f:
            safe_replace(f, name, datastream, extra_replacements, add_missing)


def zip_dir(path, dest, compression=ZIP_DEFLATED):
    with ZipFile(dest, 'w', compression) as z:
        z.add_dir(path)


__all__ = [
    'ZipFile',
    'ZipInfo',
    'ZIP_STORED',
    'ZIP_DEFLATED',
    'BadZipfile',
    'BadZipFile',
    'is_zipfile',
    'safe_replace',
    'zip_dir',
    'LargeZipFile',
    'ZipExtFile',
]
