20261027
========
First foray, setting up the project, managing versions etc.
Using a LLM to generate the syllabus, and pester with questions, 
but the code is all mine.

1. Model inference & setup
2. Data ingestion
3. Query engine & tooling

For the initial commit I got the first part working, in a cheesy Rust
shell.  The most difficult part was using the right version 
information when trying to run the actual model (was getting mixed
signals between ORT 1 and 2 based on some tutorials)

The very last and probably most important part of this assignment was 
the tail part, where each 384 fp chunk masked by the attention 
flag is spread across the accumulator, then divided by the token
count.  It seems to me that the intent here is that the prompt has
been tokenized to some n-dimensional representation, and to prepare 
for search, we're finding it's centroid and then querying for that?

Next up, setting up the db


20261028
========
Day two: ingestion
Getting sqlite up
Make the database more idempotent
Study session on Virtual tables -- sqlite interop?
[sqlite-vec](https://github.com/asg017/sqlite-vec)
ok need to init -- oh interesting the bundled feature flag is what
forces the vectorized version to be used ok cool.
OK got the data base ready for receiving chunks
Now to grab a datasource
got 13GB worth, now to write a translator
