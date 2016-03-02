# Digraph123

## Name

digraph123 - play a random walk of a directed graph made of audio files

## Synopsis

**digraph123** [-h] [--mute] [--max MAX] [path [path ...]]

### Arguments

| Argument       | Description                                                                                                              |
|----------------|--------------------------------------------------------------------------------------------------------------------------|
| *path*         | An audio file or a directory containing audio files. Files whose names don't match the *File name format* are ignored. |
| *-h*, *--help* | Show help message and exit.                                                                                              |
| *--mute*       | Don't play audio, just print filenames.                                                                                  |
| *--max=MAX*    | Maximum number of arrows to traverse.                                                                                    |

## File name format

The file name format matches the patterns TAIL-HEAD.EXT or
TAIL-HEAD-DESCRIPTION.EXT. TAIL and HEAD are node labels. DESCRIPTION is
ignored. EXT indicates the file format and is either `ogg` or `mp3`. HEAD, TAIL
and EXT are case insensitive.

## Description

**digraph123** performs the following steps:

 1. The *path* arguments are processed. An arrow is associated with each
    resulting file and a directed graph (digraph) is constructed from the
    arrows.

 2. An initial *current node* is selected. If a node has been labeled `start`,
    this node is selected. Otherwise, the tail node of a random arrow is
    selected.

 3. While the *current node* has at least one outgoing arrow, repeat:

    1. A random outgoing arrow from the *current node* is selected.

    2. The audio file associated with the selected arrow is played until it
       ends. (Unless *--mute* is specified)

    3. The head node of the selected arrow becomes the new *current node*.

Terminate **digraph123** by pressing `Ctrl+C`.

## Dependencies
 * Python 2.7
 * Pygame 1.9
