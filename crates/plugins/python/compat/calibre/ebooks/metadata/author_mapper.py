"""Mirrors the helpers of calibre's `calibre.ebooks.metadata.author_mapper`
that sources use: fixing the case of author names, and the rule-based
author map (rules applied as calibre's are)."""

import re


def cap_author_token(token):
    """Capitalize one word of an author name, keeping initials and
    particles ('van', 'de') as they are."""
    lt = token.lower()
    if lt in ('von', 'de', 'el', 'van', 'le'):
        return lt
    # If token contains period, do not change it, except for the first letter
    if lt.find('.') != -1 and len(token) > 1:
        return token[0].upper() + token[1:]
    from calibre.utils.icu import capitalize

    return capitalize(token)


def compile_rules(rules):
    """Rules: dicts with 'match_type' ('one_of', 'not_one_of', 'matches',
    'not_matches', 'has', 'is'), 'query', 'action' ('replace', 'remove',
    'split', 'capitalize', 'lower', 'upper'), 'replace'."""
    ans = []
    for rule in rules:
        mt = rule['match_type']
        q = rule['query']
        if 'matches' in mt:
            q = re.compile(q, re.IGNORECASE | re.UNICODE)
        elif mt in ('one_of', 'not_one_of'):
            q = frozenset(x.strip().lower() for x in q.split('&'))
        elif mt == 'has':
            q = q.lower()
        ans.append((rule, q))
    return ans


def matcher(rule, author):
    rule, q = rule
    mt = rule['match_type']
    lower = author.lower()
    if mt == 'one_of':
        return lower in q
    if mt == 'not_one_of':
        return lower not in q
    if mt == 'matches':
        return q.match(author) is not None
    if mt == 'not_matches':
        return q.match(author) is None
    if mt == 'has':
        return q in lower
    if mt == 'is':
        return lower == rule['query'].lower()
    return False


def apply_rules(author, rules):
    ans = []
    authors = [author]
    maxiter = 20
    while authors and maxiter > 0:
        author = authors.pop()
        ans.append(author)
        for rule, q in rules:
            if matcher((rule, q), author):
                ac = rule['action']
                if ac == 'remove':
                    ans.pop()
                    break
                if ac == 'replace':
                    if 'matches' in rule['match_type']:
                        author = q.sub(rule['replace'], author)
                    else:
                        author = rule['replace']
                    if '&' in author:
                        replacement_authors = [x.strip() for x in author.split('&')]
                        ans.pop()
                        ans.extend(replacement_authors)
                    else:
                        ans[-1] = author
                    break
                if ac == 'split':
                    stuff = [x.strip() for x in author.split(rule['replace'])]
                    ans.pop()
                    ans.extend(stuff)
                    break
                if ac == 'capitalize':
                    ans[-1] = ' '.join(map(cap_author_token, author.split()))
                    break
                if ac == 'lower':
                    ans[-1] = author.lower()
                    break
                if ac == 'upper':
                    ans[-1] = author.upper()
                    break
        maxiter -= 1
    return ans


def map_authors(authors, rules=()):
    if not authors:
        return []
    if not rules:
        return list(authors)
    ans = []
    for a in authors:
        ans.extend(apply_rules(a, rules))
    return [x for x in ans if x]
