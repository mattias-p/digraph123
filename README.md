# mazeplay
Random walks through a directed graph of music tracks. Supports the mp3 and ogg (vorbis) audio format.

## Dependencies
 * Python 2.7
 * Pygame 1.9
 
## Usage
 1. Put a bunch of files named `FROM-TO.EXT` in a directory (e.g. `dir1`). where `FROM` and `TO` are node names and `EXT` is either "mp3" or "ogg".
 2. Run the command `mazeplay dir1` (assuming that dir1 is the directory where you put your files).
 3. Stop it using `Ctrl+C`.

## Operation
 * If there is a node called "start" with out-going edges it is selected to be the start node, otherwise a random node is selected.
 * If there are no out-going edges to choose from, playback stops and the program exits.
