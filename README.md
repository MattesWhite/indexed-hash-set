# indexed-hash-set

This crate provides a bidirectional set with the following properties:
- Each entry is distinct (no double elements).
- Entries can be accessed by reference, like in a standard `HashSet`.
- Entries can be accessed via indices avoiding the cost of hashing the element.
- Indices are reference-counted. This means when no external index is around
  an entry is considered _unused_ and will be dropped on `drop_unused()`.
- Internal a [generational arena] is used to allow for effective mutation
  of the set.

## When to use

This data structure was developed to be a store for nodes in a graph. The design
allows a graph to store the indexes of nodes while nodes can be looked up by
hashing them.

## Contribution

This is only a small side project so I won't spend much time on perfecting the
code. However, I'm happy if this is used by someone else. If you have any
problems or questions file an issue or make a PR.

## License

This crate is licensed under the [MIT](./LICENSE) license.
