# whatchamacallit
A local semantic reverse dictionary
There's a lot of times I'm trying to search for the _mot juste_ but
all I have at hand is a definition.  It would be incredibly handy to
have a local tool that I could provide a rough definition for and get
a list of possible English words back from.  This also seems like a 
good learning project to understand LLMs, training, and optimization.

# Why?
Learning how to leverage and train local models for fun and no profit.

# How?
Using the [all-MiniLM-L6-v2-onnx](https://huggingface.co/onnx-models/all-MiniLM-L6-v2-onnx/tree/main) model, 
and experimenting with an LLM generated syllabus.

## File structure

`models/`
Local ONNX models go here (currently `all-MiniLM-L6-v2`).

`db/`
sqlite3 db for storing and querying the vectors

`dictionary/`
raw dictionary goes here for processing
