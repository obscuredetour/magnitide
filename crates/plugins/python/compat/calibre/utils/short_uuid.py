"""Mirrors calibre's `calibre.utils.short_uuid`: UUIDs as short strings of
URL-safe characters (22 instead of 36)."""

import string
import uuid as _uuid

_alphabet = string.ascii_letters + string.digits + '-_'
_alphabet_len = len(_alphabet)


def num_to_string(number, alphabet, alphabet_len, pad_to_length=None):
    ans = []
    while number:
        number, digit = divmod(number, alphabet_len)
        ans.append(alphabet[digit])
    if pad_to_length is not None and pad_to_length > len(ans):
        ans.append(alphabet[0] * (pad_to_length - len(ans)))
    return ''.join(ans)


def string_to_num(string, alphabet_map, alphabet_len):
    num = 0
    for char in reversed(string):
        num = num * alphabet_len + alphabet_map[char]
    return num


def uuid4():
    """A short random UUID."""
    return num_to_string(_uuid.uuid4().int, _alphabet, _alphabet_len, 22)


def uuid_to_short(u):
    if isinstance(u, str):
        u = _uuid.UUID(u)
    return num_to_string(u.int, _alphabet, _alphabet_len, 22)


def short_to_uuid(short):
    m = {c: i for i, c in enumerate(_alphabet)}
    return _uuid.UUID(int=string_to_num(short, m, _alphabet_len))


if __name__ == '__main__':
    print(uuid4())
