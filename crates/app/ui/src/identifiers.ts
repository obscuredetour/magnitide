// The order book identifiers are listed in: the ones people look for first.

const FIRST = ['isbn', 'asin'];

const rank = (scheme: string) => {
  const i = FIRST.indexOf(scheme.toLowerCase());
  return i < 0 ? FIRST.length : i;
};

/** Sort `[scheme, value]` pairs with ISBN and ASIN first; the rest keep their order. */
export const identifierOrder = ([a]: [string, unknown], [b]: [string, unknown]) => rank(a) - rank(b);
