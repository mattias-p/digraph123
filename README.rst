===========
digraph123_
===========

Play digraph_ shaped audio recordings using random walk.


Introduction
------------
A traditional audio recording has a single timeline from start to end. In the
language of directed graphs (digraphs) this can be described using two nodes
labeled "start" and "end" and an arrow going from "start" to "end". Another type
of audio recording combines an intro part and a loop part. This can be described
using two nodes labeled "start" and "loop" and two arrows - one going from
"start" to "loop" and another going from "loop" back onto itself.

*Digraph shaped audio recording* generalizes the above concept to the set of all
non-empty digraphs with one of their nodes designated the *start node*.

**digraph123** defines a format for digraph shaped audio recordings and
traverses recordings in this format from the start node using random walk. In
its default mode of operation it plays the part associated with each traversed
arrow. Alternatively it can be muted to generate playlists of the traversed
parts at a rate not limited by the playback of individual parts.


Installation
------------

1. Make sure the following dependencies are installed:

   * Python 2.7
   * Pygame 1.9

2. Download and extract a ZIP of the current version from `here
   <https://github.com/mattias-p/digraph123>`_.

3. Add **digraph123** to your PATH_.


Gettings started
----------------
**digraph123** is a command line tool, so start out by opening a console_.


Play an example
~~~~~~~~~~~~~~~
I'd recommend a recording to try out, but I don't know of any. The best I can do
is to direct you to create your own non-linear medley example.

1. Create a directory called ``medley-example``.

2. Download a dozen tracks of less than ten seconds each into ``medley-example``
   from https://www.jamendo.com/community/short/tracks.

3. Rename the files to:

   * down-north.mp3
   * down-south.mp3
   * east-down.mp3
   * east-up.mp3
   * north-east.mp3
   * north-west.mp3
   * south-east.mp3
   * south-west.mp3
   * up-north.mp3
   * up-south.mp3
   * west-down.mp3
   * west-up.mp3

4. Play it::

     digraph123 path/to/medley-example

   First, feedback about all found arrows are printed to stderr. This can be
   used to verify that the constructed digraph matches your expectations.

   As an arrow begins to be traversed, the path of its associated audio file is
   printed to stdout. This can be used to store a playlist of the traversal.


Learn more
~~~~~~~~~~
Use the ``--help`` option for details on usage and operation::

  digraph123 --help


Troubleshooting
---------------
I get complains about too few arguments:
  digraph123 expects a path to a set of audio files as an argument. See the
  Getting started section for an example.


Contributing
------------
* Post bug reports and/or feature requests to the `issue tracker`_.
* Compose, record and publish and spread the word about digraph shaped audio.
* Look through the `issue tracker`_.
  for things to do and make pull requests to the `master branch`_.
* Have fun with it.


License
-------
| digraph123 plays digraph shaped audio recordings.
| Copyright (C) 2016  Mattias Päivärinta
|
| This program is free software: you can redistribute it and/or modify
| it under the terms of the GNU General Public License as published by
| the Free Software Foundation, either version 3 of the License, or
| (at your option) any later version.
|
| This program is distributed in the hope that it will be useful,
| WITHOUT ANY WARRANTY; without even the implied warranty of
| MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
| GNU General Public License for more details.
|
| You should have received a copy of the GNU General Public License
| along with this program.  If not, see <http://www.gnu.org/licenses/>.


TODO
----

1. Getting started

   * Include example recording
   * Rewrite getting started section

2. Installation

   * Split code into script and library
   * Proper pip support
   * Create a test suite
   * Include Makefile
   * Update installation instruction

3. Documentation

   * Document library and script
   * Support Sphinx

4. Code

   * Use pep8
   * Look into Click


.. _console:       https://en.wikipedia.org/wiki/Command-line_interface
.. _digraph123:    https://github.com/mattias-p/digraph123
.. _digraph:       https://en.wikipedia.org/wiki/Directed_graph
.. _issue tracker: https://github.com/mattias-p/digraph123/issues
.. _master branch: https://github.com/mattias-p/digraph123/tree/master
.. _PATH:          https://en.wikipedia.org/wiki/PATH_(variable)
