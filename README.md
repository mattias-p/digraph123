digraph123
==========

Play audio recordings shaped like [directed graphs](https://en.wikipedia.org/wiki/Directed_graph)
(digraphs) by means of random walk. Contrast this to the traditional playing of
linear audio recordings from start to end.

Project homepage: https://github.com/mattias-p/digraph123


Installation
------------

Make sure the following dependencies are installed:
 * Python 2.7
 * Pygame 1.9

Download and extract a ZIP of the current version from [here](https://github.com/mattias-p/digraph123).

Add **digraph123** to your [PATH](https://en.wikipedia.org/wiki/PATH_%28variable%29).


Gettings started
----------------

**digraph123** is a command line tool, so start out by opening a [console](https://en.wikipedia.org/wiki/Command-line_interface).


### Play an example

I'd recommend a recording to try out, but I don't know of any. The best I can do
is to direct you to create your own non-linear medley example.

 1. Create a directory called `medley-example`.

 2. Download a dozen tracks of less than ten seconds each into `medley-example`
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

 4. Play it:

        digraph123 path/to/medley-example


### Learn more

Use the `--help` option for details on usage and operation:

    digraph123 --help


Troubleshooting
---------------

<dl>
  <dt>I get complains about too few arguments:</dt>
  <dd>digraph123 expects a path to a set of audio files as an argument. See the
  Getting started section for an example.</dd>
</dl>


Contributing
------------

### As a user:

 * Post bug reports and/or feature requests to the [issue tracker](https://github.com/mattias-p/digraph123/issues).
 * Have fun with it.


### As a musician:

 * Compose, record and publish digraph shaped audio.
 * Spread the word about digraph shaped audio.
 * Have fun with it.


### As a coder:

 * Look through the [issue tracker](https://github.com/mattias-p/digraph123/issues)
   for things to do.
 * Make pull requests at the [master branch](https://github.com/mattias-p/digraph123/tree/master).
 * Have fun with it.


License
-------
```
digraph123 plays digraph shaped audio recordings.
Copyright (C) 2016  Mattias Päivärinta

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <http://www.gnu.org/licenses/>.
```
