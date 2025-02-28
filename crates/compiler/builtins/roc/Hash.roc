interface Hash
    exposes [
        Hash,
        Hasher,
        hash,
        addBytes,
        addU8,
        addU16,
        addU32,
        addU64,
        addU128,
        hashI8,
        hashI16,
        hashI32,
        hashI64,
        hashI128,
        complete,
        hashStrBytes,
        hashList,
        hashUnordered,
    ] imports [
        List,
        Str,
        Num.{ U8, U16, U32, U64, U128, I8, I16, I32, I64, I128 },
    ]

## A value that can hashed.
Hash has
    ## Hashes a value into a [Hasher].
    ## Note that [hash] does not produce a hash value itself; the hasher must be
    ## [complete]d in order to extract the hash value.
    hash : hasher, a -> hasher | a has Hash, hasher has Hasher

## Describes a hashing algorithm that is fed bytes and produces an integer hash.
##
## The [Hasher] ability describes general-purpose hashers. It only allows
## emission of 64-bit unsigned integer hashes. It is not suitable for
## cryptographically-secure hashing.
Hasher has
    ## Adds a list of bytes to the hasher.
    addBytes : a, List U8 -> a | a has Hasher

    ## Adds a single U8 to the hasher.
    addU8 : a, U8 -> a | a has Hasher

    ## Adds a single U16 to the hasher.
    addU16 : a, U16 -> a | a has Hasher

    ## Adds a single U32 to the hasher.
    addU32 : a, U32 -> a | a has Hasher

    ## Adds a single U64 to the hasher.
    addU64 : a, U64 -> a | a has Hasher

    ## Adds a single U128 to the hasher.
    addU128 : a, U128 -> a | a has Hasher

    ## Completes the hasher, extracting a hash value from its
    ## accumulated hash state.
    complete : a -> U64 | a has Hasher

## Adds a string into a [Hasher] by hashing its UTF-8 bytes.
hashStrBytes = \hasher, s ->
    addBytes hasher (Str.toUtf8 s)

## Adds a list of [Hash]able elements to a [Hasher] by hashing each element.
hashList = \hasher, lst ->
    List.walk lst hasher \accumHasher, elem ->
        hash accumHasher elem

## Adds a single I8 to a hasher.
hashI8 : a, I8 -> a | a has Hasher
hashI8 = \hasher, n -> addU8 hasher (Num.toU8 n)

## Adds a single I16 to a hasher.
hashI16 : a, I16 -> a | a has Hasher
hashI16 = \hasher, n -> addU16 hasher (Num.toU16 n)

## Adds a single I32 to a hasher.
hashI32 : a, I32 -> a | a has Hasher
hashI32 = \hasher, n -> addU32 hasher (Num.toU32 n)

## Adds a single I64 to a hasher.
hashI64 : a, I64 -> a | a has Hasher
hashI64 = \hasher, n -> addU64 hasher (Num.toU64 n)

## Adds a single I128 to a hasher.
hashI128 : a, I128 -> a | a has Hasher
hashI128 = \hasher, n -> addU128 hasher (Num.toU128 n)

## Adds a container of [Hash]able elements to a [Hasher] by hashing each element.
## The container is iterated using the walk method passed in.
## The order of the elements does not affect the final hash.
hashUnordered = \hasher, container, walk ->
    walk
        container
        0
        (\accum, elem ->
            x =
                # Note, we intentionally copy the hasher in every iteration.
                # Having the same base state is required for unordered hashing.
                hasher
                |> hash elem
                |> complete
            nextAccum = Num.addWrap accum x

            if nextAccum < accum then
                # we don't want to lose a bit of entropy on overflow, so add it back in.
                Num.addWrap nextAccum 1
            else
                nextAccum
        )
    |> \accum -> addU64 hasher accum
