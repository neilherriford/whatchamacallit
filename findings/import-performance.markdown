# Overview
This document contains a report of findings and improvements made
during implementation. The objective is to take an XML dump file from
the Wiktionary project, extract the English words with definitions, and
populate a SQLite database that will be used to train the LLM.

Extracting the words and definitions is unfortunately not achievable
directly from the dump. Templates are widely used to help Wiktionary
human editors work, but make rote extraction unfeasible. The solution
is to load the template definitions into a local MediaWiki instance and
render the definitions there. This creates the following pipeline:

1. Load "enough" of the templates into a local MediaWiki instance
2. Page through the dump file, looking for entries in namespace 0 (the
   definitions)
3. Filter out pages that lack an English section
4. Extract the definition and antonym markup
5. Send the markup to MediaWiki to render
6. Extract the plain text from the HTML MediaWiki response
7. Insert into the database.

Finding "enough" templates was accomplished by trial and error.  For
definitions, these were found to be required for rendering:

| Namespace | Name        | Pages     | Description                   | How it's used                                                                                                                                                      |
| ---:      | ---         | ---:      | ---                           | ---                                                                                                                                                                |
| 0         | Main        | 9,398,297 | Dictionary entries            | Used because some entries are interdependent to render, e.g. `Module:transclude` and `Module:senseno` read other entries, and `{{vern}}` checks if an entry exists |
| 8         | MediaWiki   |     1,159 | Interface message pages       | Without it the reference back-link renders as `↑` instead of `^`                                                                                                   |
| 10        | Template    |    74,584 | Template pages                | A missing template renders as red-link text `Template:name`                                                                                                        |
| 828       | Module      |    63,156 | Lua modules used by templates | This is where the Lua source handles calls to `{{#invoke:…}}`                                                                                                      |
| 100       | Appendix    |    31,886 | Appendix pages                | Label and Ancient Greek link output depends on these pages                                                                                                         |

The naive approach is to load the above namespaces first, then iterate
definition by definition into the database.  However restoring these
templates takes about 4.87 hours.  There are 1,804,774 English
definitions in this dump file, and rendering each definition one at a
time would take a projected 62.10 hours.

While this is a one time cost, there has got to be a more performant
way of loading all of these records.  It is quite likely that during
the course of development, an import may need to be rerun.

## Extracting and rendering Wiktionary entries
My first approach was to try and implement local rendering of templates
as I ran across them, creating a deny list to skip extraneous templates.
This approach was tested against a few sporadic words and definitions,
and divergence was immediately present.

Since the hosting solution of Wiktionary is also open source, a better
approach seemed to be to host enough of the Wiktionary system to render
the definitions.  This led to two additional problems.

### Populate MediaWiki Server
It may be possible to do some static analysis and determine which pages
and namespaces need to be loaded to support a full corpus, but it is
safer (and easier) just to load enough namespaces until everything
renders, then focus on speeding that process up.

#### Baseline

| Metric                    |                        Baseline |
| ---                       | ---:                            |
| Workers                   |                            1    |
| Parts                     |                             n/a |
| DB profile                | default (assumed, not recorded) |
| Imported pages            |                    9,570,595    |
| Share of full import      |                          100.0% |
| Import seconds            |                       17,530    |
| Imported pages per second |                          546.0  |
| Projected total time (h)  |                            4.87 |
| Improvement over baseline |                             n/a |

The original import log reports 645.63 pages/sec, but that appears to
count every page read from the dump (about 11.3 million), not the pages
imported.  The rate here is recomputed on imported pages, so 546.0 is
comparable with every later row.  All runs used Docker with 12 CPUs and
16GB of memory.

#### Test 1: Split with one worker
First step is to proof-of-concept creating the split files.  The same
step also moves the database from the default settings to the `bulk`
profile (`innodb_flush_log_at_trx_commit=2`, `doublewrite` off, 2GB log
file), so the 2.5% cannot be credited to either change.

The bulk profile provides speed over crash safety, which is what we
need.  The log is flushed once per second instead of at every commit,
and `doublewrite` buffer is off.  A crash means starting from scratch,
which for this use case is fine.

As noted above, only the namespaces _Main_, _MediaWiki_, _Template_,
_Module_, and _Appendix_ are needed to render the definitions.  To
import in parallel, the dump is split into parts and multiple processes
run at once.  The `split-dump` command scans the dump file into ranges
to be imported.

| Metric                    |     Baseline |     Test 1 |
| ---                       | ---:         | ---:       |
| Workers                   |         1    |       1    |
| Parts                     |          n/a |      12    |
| DB profile                |      default |       bulk |
| Imported pages            | 9,570,595    | 865,146    |
| Share of full import      |       100.0% |       9.0% |
| Import seconds            |    17,530    |   1,545.6  |
| Imported pages per second |       546.0  |     559.8  |
| Projected total time (h)  |         4.87 |       4.75 |
| Improvement over baseline |          n/a |       2.5% |

#### Test 2: Split with two workers
Double the workers!

| Metric                    |     Test 1 |     Test 2 |
| ---                       | ---:       | ---:       |
| Workers                   |       1    |       2    |
| Parts                     |      12    |      12    |
| DB profile                |       bulk |       bulk |
| Imported pages            | 865,146    | 865,146    |
| Share of full import      |       9.0% |       9.0% |
| Import seconds            |   1,545.6  |     885.6  |
| Imported pages per second |     559.8  |     976.9  |
| Projected total time (h)  |       4.75 |       2.72 |
| Improvement over baseline |       2.5% |      44.1% |

#### Test 3: Split with four workers
Double again!

| Metric                    |     Test 2 |     Test 3 |
| ---                       | ---:       | ---:       |
| Workers                   |       2    |       4    |
| Parts                     |      12    |      12    |
| DB profile                |       bulk |       bulk |
| Imported pages            | 865,146    | 865,146    |
| Share of full import      |       9.0% |       9.0% |
| Import seconds            |     885.6  |     534.1  |
| Imported pages per second |     976.9  |   1,619.8  |
| Projected total time (h)  |       2.72 |       1.64 |
| Improvement over baseline |      44.1% |      66.3% |

#### Test 4: Split with eight workers
Keep that geometric sequence going!

| Metric                    |     Test 3 |     Test 4 |
| ---                       | ---:       | ---:       |
| Workers                   |       4    |       8    |
| Parts                     |      12    |      12    |
| DB profile                |       bulk |       bulk |
| Imported pages            | 865,146    | 865,146    |
| Share of full import      |       9.0% |       9.0% |
| Import seconds            |     534.1  |     341.1  |
| Imported pages per second |   1,619.8  |   2,536.3  |
| Projected total time (h)  |       1.64 |       1.05 |
| Improvement over baseline |      66.3% |      78.5% |

#### Test 5: Split with 12 workers
The performance gains are slowing, test with another worker bump.
Twelve is also the number of CPUs available.

| Metric                    |     Test 4 |     Test 5 |
| ---                       | ---:       | ---:       |
| Workers                   |       8    |      12    |
| Parts                     |      12    |      12    |
| DB profile                |       bulk |       bulk |
| Imported pages            | 865,146    | 865,146    |
| Share of full import      |       9.0% |       9.0% |
| Import seconds            |     341.1  |     270.8  |
| Imported pages per second |   2,536.3  |   3,194.8  |
| Projected total time (h)  |       1.05 |       0.83 |
| Improvement over baseline |      78.5% |      82.9% |

#### Test 6: Split with 16 workers, and tune the db
The 16 worker run on the `bulk` profile failed.  MariaDB 11.8 defaults
to `innodb_snapshot_isolation=1`, so concurrent importers conflicted
when creating actor rows.  The `bulk-si` profile turns that off.  This
ran twice and the two runs differ by 18.7%, so 16 workers is no better
than 12 beyond the noise.

| Metric                    |     Test 5 | Test 6, run A | Test 6, run B |
| ---                       | ---:       | ---:          | ---:          |
| Workers                   |      12    |         16    |         16    |
| Parts                     |      12    |         12    |         12    |
| DB profile                |       bulk |       bulk-si |       bulk-si |
| Imported pages            | 865,146    |    865,146    |    865,146    |
| Share of full import      |       9.0% |          9.0% |          9.0% |
| Import seconds            |     270.8  |        309.0  |        260.3  |
| Imported pages per second |   3,194.8  |      2,799.8  |      3,323.7  |
| Projected total time (h)  |       0.83 |          0.95 |          0.80 |
| Improvement over baseline |      82.9% |         80.5% |         83.6% |

#### Test 7: More splits
A sample with 12 parts is limited by its slowest part, so use more,
smaller parts and a bigger sample.  This step also moves to the
`bulk-si` profile.

| Metric                    |     Test 5 |       Test 7 |
| ---                       | ---:       | ---:         |
| Workers                   |      12    |        12    |
| Parts                     |      12    |        60    |
| DB profile                |       bulk |      bulk-si |
| Imported pages            | 865,146    | 4,683,417    |
| Share of full import      |       9.0% |        48.9% |
| Import seconds            |     270.8  |     1,008.3  |
| Imported pages per second |   3,194.8  |     4,644.9  |
| Projected total time (h)  |       0.83 |         0.57 |
| Improvement over baseline |      82.9% |        88.2% |

#### Test 8: Even more splits
This is the measured full import, not a projection.  The cold start
went from 4.87 hours to about 28 minutes.

| Metric                    |       Test 7 |       Test 8 |
| ---                       | ---:         | ---:         |
| Workers                   |        12    |        12    |
| Parts                     |        60    |       120    |
| DB profile                |      bulk-si |      bulk-si |
| Imported pages            | 4,683,417    | 9,570,595    |
| Share of full import      |        48.9% |       100.0% |
| Import seconds            |     1,008.3  |     1,678    |
| Imported pages per second |     4,644.9  |     5,703.6  |
| Projected total time (h)  |         0.57 |         0.47 |
| Improvement over baseline |        88.2% |        90.4% |

#### Test 9: Are all the namespaces really needed?
Trying some experiments removing namespaces until things broke.
Previously, `829` was being loaded here, but it turns out nothing
actually uses those.

| Metric                    |       Test 8 |       Test 9 |
| ---                       | ---:         | ---:         |
| Workers                   |        12    |        12    |
| Parts                     |       120    |       120    |
| DB profile                |      bulk-si |      bulk-si |
| Imported pages            | 9,570,595    | 9,569,082    |
| Share of full import      |       100.0% |       100.0% |
| Import seconds            |     1,678    |     1,631    |
| Imported pages per second |     5,703.6  |     5,867.0  |
| Projected total time (h)  |         0.47 |         0.45 |
| Improvement over baseline |        90.4% |        90.7% |

Removing 1,513 pages doesn't hugely change the overall timing, but it
does keep things simple.  An earlier import without namespaces 8 and
829 took 1,543 s, and dump scans ran during part of the 1,631 s import,
so 1,543, 1,631 and 1,678 s are one noisy band.

Next the test was to remove the biggest, namespace zero.  The experiment
was to run a full import and render then count the number of errors.

| Metric                   | With namespace 0 | Without namespace 0 |
| ---                      | ---:             | ---:                |
| Import seconds           |    1,631         |    334              |
| Rendered definitions     |    1,804,587     |    1,804,023        |
| Failed definitions       |    187           |    751              |
| Newly failed definitions |    n/a           |    573              |

Enough tripped here that it's worth keeping in.

Namespace 0 stays.  `Module:transclude` and `Module:transclude_def`
failed on 363 definitions ("Couldn't find the entry"), `Module:senseno`
failed on 211 ("Could not read content of page"), and 9 definitions that
failed on a `{{vern}}` warning in the full import now rendered without
any error, because the page it checks for no longer exists.

#### Estimates against the measured run
The 12-part samples overstate the time because the slowest part limits
them.  Five runs are left out of these tables: three that failed from a
race creating the system user (`e2-bulk-w2`, `w4`, `w8`), the 16 worker
run on `bulk` that failed, and a 60-part run that ran out of Docker
disk.

| Metric                   | Test 5 (12 parts) | Test 7 (60 parts) | Full run |
| ---                      | ---:              | ---:              | ---:     |
| Projected total time (h) |              0.83 |              0.57 |     0.47 |
| Difference from measured |            +78.5% |            +22.8% |      n/a |

### Rendering performance
After I finished writing a simple parser which divided the markup into
chunks, the naive solution was to submit one definition at a time to
the MediaWiki back end to render into HTML markup, and then use a crate
to extract the `innerText` from that result.

Example, the fifth definition for _nonsense_

__Markup__
```plaintext
# {{lb|en|literature}} A type of poetry that contains strange or surreal ideas, as, for example, that written by {{w|Edward Lear}}.
```

__JSON Response__
```json
{
  "parse":
  {
    "title": "nonsense",
    "pageid": 31,
    "text": "<ol><li><span class=\"usage-label-sense\"><span class=\"ib-brac label-brac\">(</span><span class=\"ib-content label-content\"><a href=\"/index.php/literature\" title=\"literature\">literature</a></span><span class=\"ib-brac label-brac\">)</span></span> A type of poetry that contains strange or surreal ideas, as, for example, that written by <a href=\"/index.php?title=w:Edward_Lear&amp;action=edit&amp;redlink=1\" class=\"new\" title=\"w:Edward Lear (page does not exist)\">Edward Lear</a>.</li></ol>"
  }
}
```

__Plain text extraction__
```plaintext
(literature) A type of poetry that contains strange or surreal ideas, as, for example, that written by Edward Lear.
```

#### Determining the baseline

For the first pass, I ran the program against every 1,000th page in the
English dump, added some instrumentation around the different stages
and ran it against the dataset.  So, for the 9.2 million pages it
sampled 9,246, and of those, 1,396 had actual definitions.  The rest
were placeholders, or lacked an English section, etc.

IO reading through the multi gigabyte dump file dominates when the
sample size is small, so the method for calculating projected hours is
as follows:

Σ `request_seconds` ÷ `concurrency` ÷ `definitions` × 1,804,774 ÷ 3,600

Where 1,804,774 is the number of non-blank English definitions in the
13.9GB dump file at the time of writing. The findings showed:

| Metric                    | Run 1 (baseline) |
| ---                       | ---:             |
| Concurrency               |     1            |
| Words per request         |      n/a         |
| Definitions               | 1,781            |
| Stride                    | 1,000            |
| Render seconds            |   220.6          |
| Projected total time (h)  |    62.10         |
| Improvement over baseline |      n/a         |

Almost three days to import the dataset is unacceptable.  What could be
taking so long?  Looking at my instrumentation results:

| Stage        | %      |
| ---          | ---:   |
| render       | 99.98  |
| page load    |  0.004 |
| parse markup |  0.010 |
| parse entry  |  0.002 |

The data revealed that parsing, and navigating through a multi-gig
dump file took up almost no time versus the actual rendering work.  So
I started looking into alternatives, re-ran the tests, and compared
against the baseline.

#### Run 2: Word at a time
It seemed like a lot of the time in the request was spent starting the
new Lua engine over-and-over.  To verify that, my first approach was to
start batching requests by word instead of by definition.  I used a
clever separator (reflecting back on the `SEPARATOR` control characters
of yore) that would be easy to split on and extremely unlikely to be
part of any corpus.

| Metric                    | Run 1    | Run 2    |
| ---                       | ---:     | ---:     |
| Concurrency               | 1        |     1    |
| Words per request         |      n/a |     1    |
| Definitions               | 1,781    | 1,781    |
| Stride                    | 1,000    | 1,000    |
| Render seconds            |   220.6  |   175.3  |
| Projected total time (h)  |    62.10 |    49.34 |
| Improvement over baseline |      n/a |    20.5% |

This is an improvement but it's still too slow!

#### Run 3: Multiple words at a time
The next logical thought is to increase the batch size, to multiple
words.  A concern here is that some pages rely on the `title` query
variable being set, which is the word itself.  Doing some static
analysis on the dump file on which templates need that variable, and
which words in turn use that variable, it is possible to sort the words
into batchable and non-batchable.  The same technique as run two was
used now making it simple to separate whole words.

| Metric                    | Run 2    | Run 3    |
| ---                       | ---:     | ---:     |
| Concurrency               |     1    |     1    |
| Words per request         |     1    |    10    |
| Definitions               | 1,781    | 1,781    |
| Stride                    | 1,000    | 1,000    |
| Render seconds            |   175.3  |   162.8  |
| Projected total time (h)  |    49.34 |    45.83 |
| Improvement over baseline |    20.5% |    26.2% |

This improvement was less than expected, due partially to words that
require their own title to render properly.

#### Run 4: Multiple words and parallel requests
Ok well, maybe parallelism will help here?  Let's take the batch of 10
words, and make 10 simultaneous requests of each.

| Metric                    | Run 3    | Run 4    |
| ---                       | ---:     | ---:     |
| Concurrency               |     1    |    10    |
| Words per request         |    10    |    10    |
| Definitions               | 1,781    | 1,781    |
| Stride                    | 1,000    | 1,000    |
| Render seconds            |   162.8  |   208.2  |
| Projected total time (h)  |    45.83 |     5.86 |
| Improvement over baseline |    26.2% |    90.6% |

Render seconds rose from 162.8 to 208.2 because each request now waits
on a busier server.  The projection divides by the 10 requests in
flight, which is why it fell.

This is a great improvement in the projected time, can we squeeze more?

#### Run 5: Larger parallel requests
[Yes!](https://en.wikipedia.org/wiki/Betteridge%27s_law_of_headlines)

| Metric                    | Run 4    | Run 5    |
| ---                       | ---:     | ---:     |
| Concurrency               |    10    |    10    |
| Words per request         |    10    |    50    |
| Definitions               | 1,781    | 1,781    |
| Stride                    | 1,000    | 1,000    |
| Render seconds            |   208.2  |    43.3  |
| Projected total time (h)  |     5.86 |     1.22 |
| Improvement over baseline |    90.6% |    98.0% |

Still not fast enough!

#### Run 6: Template caching
Could an overly clever caching scheme help here?  Keeping the same
settings as run 5, but this time split each definition into its template
calls and the text between them, and render each distinct piece only
once per run.  Also increase the sample size, now testing every 100th
page.

| Metric                    | Run 6     |
| ---                       | ---:      |
| Concurrency               |     10    |
| Words per request         |     50    |
| Definitions               | 18,213    |
| Stride                    |    100    |
| Render seconds            |    464.5  |
| Projected total time (h)  |      1.28 |
| Improvement over baseline |     97.9% |

This skipped 29.2% of the pieces, but render seconds were only 3.4%
lower than run 7 below (464.5 against 481.0), which uses the same sample
without it.  This was just a proof of concept test, and rebuilding each
definition from separately rendered pieces has a lot of complications,
which makes it risky.

#### Run 7 & 8: Server tuning, baseline with new sample size.
Shelving that approach for now, let's focus instead on server tuning.

| Metric                    | Run 7     | Run 8     |
| ---                       | ---:      | ---:      |
| Concurrency               |     10    |     10    |
| Words per request         |     50    |     50    |
| Definitions               | 18,213    | 18,213    |
| Stride                    |    100    |    100    |
| Render seconds            |    481.0  |    470.5  |
| Projected total time (h)  |      1.32 |      1.30 |
| Improvement over baseline |     97.9% |     97.9% |

The results were a little noisy so the test was re-run, showing a 2.2%
difference.

#### Run 9: Use the Lua sandbox
Switch Scribunto from `luastandalone` to `luasandbox`.  The sandbox runs
inside the PHP process instead of starting a separate Lua process, which
probably accounts for the drop, but I did not measure why.

| Metric                    | Run 8     | Run 9     |
| ---                       | ---:      | ---:      |
| Concurrency               |     10    |     10    |
| Words per request         |     50    |     50    |
| Definitions               | 18,213    | 18,213    |
| Stride                    |    100    |    100    |
| Render seconds            |    470.5  |    111.0  |
| Projected total time (h)  |      1.30 |      0.31 |
| Improvement over baseline |     97.9% |     99.5% |

This dramatically increased performance, render seconds are dropped by
more than 75%.

#### Run 10: Bump InnoDB buffer
Next, try increasing the buffer pool from 128MB to 4GB.

| Metric                    | Run 9     | Run 10    |
| ---                       | ---:      | ---:      |
| Concurrency               |     10    |     10    |
| Words per request         |     50    |     50    |
| Definitions               | 18,213    | 18,213    |
| Stride                    |    100    |    100    |
| Render seconds            |    111.0  |    106.7  |
| Projected total time (h)  |      0.31 |      0.29 |
| Improvement over baseline |     99.5% |     99.5% |

A 3.9% drop in render seconds.  The repeat of runs 7 and 8 differed by
2.2%, so this is within the margin of error or close to it.

#### Run 11: Opcache tuning
Now, testing out options in PHP itself, set the cache to 256MB, disable
timestamp validation.

| Metric                    | Run 10    | Run 11    |
| ---                       | ---:      | ---:      |
| Concurrency               |     10    |     10    |
| Words per request         |     50    |     50    |
| Definitions               | 18,213    | 18,213    |
| Stride                    |    100    |    100    |
| Render seconds            |    106.7  |    104.6  |
| Projected total time (h)  |      0.29 |      0.29 |
| Improvement over baseline |     99.5% |     99.5% |

Within the margin of error for total improvement.

#### Run 12: Bump APCu
More memory is more better?

| Metric                    | Run 11    | Run 12    |
| ---                       | ---:      | ---:      |
| Concurrency               |     10    |     10    |
| Words per request         |     50    |     50    |
| Definitions               | 18,213    | 18,213    |
| Stride                    |    100    |    100    |
| Render seconds            |    104.6  |    103.2  |
| Projected total time (h)  |      0.29 |      0.28 |
| Improvement over baseline |     99.5% |     99.5% |

Not outside the margin of error unfortunately.

#### Run 13 & 14: Re-baseline
Now the sample size is increased to get a clearer picture, capturing 10%
of all definitions.  Again this was repeated to check for noise, and
again this was within the margin of error.  Note that the sample rate
changed here which accounts for the differences from the previous run.

| Metric                    | Run 13     | Run 14     |
| ---                       | ---:       | ---:       |
| Concurrency               |      10    |      10    |
| Words per request         |      50    |      50    |
| Definitions               | 181,416    | 181,416    |
| Stride                    |      10    |      10    |
| Render seconds            |   1,194.8  |   1,161.2  |
| Projected total time (h)  |       0.33 |       0.32 |
| Improvement over baseline |      99.5% |      99.5% |

The slight increase here is due to overhead of the larger sample.

#### Run 15: 20 requests in flight
Double the number of parallel requests, but leave everything else the
same.

| Metric                    | Run 14     | Run 15     |
| ---                       | ---:       | ---:       |
| Concurrency               |      10    |      20    |
| Words per request         |      50    |      50    |
| Definitions               | 181,416    | 181,416    |
| Stride                    |      10    |      10    |
| Render seconds            |   1,161.2  |   2,197.4  |
| Projected total time (h)  |       0.32 |       0.30 |
| Improvement over baseline |      99.5% |      99.5% |

Not a super significant improvement, but every little bit helps.

#### Run 16: 40 requests in flight
Ok, now let's continue to increase concurrency.  When will it break?

| Metric                    | Run 15     | Run 16     |
| ---                       | ---:       | ---:       |
| Concurrency               |      20    |      40    |
| Words per request         |      50    |      50    |
| Definitions               | 181,416    | 181,416    |
| Stride                    |      10    |      10    |
| Render seconds            |   2,197.4  |   4,331.3  |
| Projected total time (h)  |       0.30 |       0.30 |
| Improvement over baseline |      99.5% |      99.5% |

Render seconds doubled with the concurrency while the projected time did
not change, so the server was already saturated at 20.

#### Run 17: 20 requests in flight, but 100 words per request
Increasing the concurrency seems to saturate the server now, so let's
increase the payload.

| Metric                    | Run 16     | Run 17     |
| ---                       | ---:       | ---:       |
| Concurrency               |      40    |      20    |
| Words per request         |      50    |     100    |
| Definitions               | 181,416    | 181,416    |
| Stride                    |      10    |      10    |
| Render seconds            |   4,331.3  |   1,704.0  |
| Projected total time (h)  |       0.30 |       0.24 |
| Improvement over baseline |      99.5% |      99.6% |

#### Run 18: 20 requests in flight, but 200 words per request
Bigger payload!

| Metric                    | Run 17     | Run 18     |
| ---                       | ---:       | ---:       |
| Concurrency               |      20    |      20    |
| Words per request         |     100    |     200    |
| Definitions               | 181,416    | 181,416    |
| Stride                    |      10    |      10    |
| Render seconds            |   1,704.0  |   1,517.6  |
| Projected total time (h)  |       0.24 |       0.21 |
| Improvement over baseline |      99.6% |      99.7% |

This seems to be about reaching the limits of performance without taking
on risky caching techniques, and we got it down from almost 3 days to
an estimated 13 minutes.  The measured wall-clock for the full run was
13.4 minutes.

#### Full run test
| Metric                     | Run 18     | Final, run 1 | Final, run 3 |
| ---                        | ---:       | ---:         | ---:         |
| Concurrency                |      20    |        20    |        20    |
| Definitions                | 181,416    | 1,804,774    | 1,804,774    |
| Stride                     |      10    |         1    |         1    |
| Words per request          |     200    |       200    |       200    |
| Render seconds             |   1,517.6  |    17,690.6  |    15,917.3  |
| Projected total time (h)   |       0.21 |         0.25 |         0.22 |
| Improvement over baseline  |      99.7% |        99.6% |        99.6% |
| Difference from estimation |        n/a |        17.2% |         5.4% |
| Failed definitions         |        n/a |          183 |          187 |

Run 18 used the stats harness, which lost 3.37% of words from failed
batches.  Both final runs used the render pipeline with error location,
which lost 183 definitions (0.010%) in run 1 and 187 (0.010%) in run 3.
The two code paths are different.

Run 1 used the six-namespace import.  Run 3 used namespaces 0, 8, 10,
828 and 100.  Compared by word and definition number, 1,804,567
definitions are identical.  16 differ only by the current date inside
the text, and 4 are the definitions that call templates missing from
the dump (February #0, peso #1, dredging box #0, didactically #0).
Run 1 shipped `Template:...` red-link text for those four, and the
pipeline now fails them as "Missing template".

Three full renders of essentially the same work took 17,690.6, 16,591.8
and 15,917.3 render seconds, an 11% spread.  That spread is why the
difference from the estimate is 17.2% for run 1 and 5.4% for run 3.

Not bad!
