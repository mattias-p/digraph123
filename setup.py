# -*- coding: utf-8 -*-

from setuptools import setup, find_packages


with open('README.rst') as f:
    readme = f.read()

with open('LICENSE') as f:
    license = f.read()

setup(
    name='digraph123',
    version='0.0.1',
    description='Play digraph shaped audio recordings using random walk',
    long_description=readme,
    author='Mattias Päivärinta',
    author_email='mattias.paivarinta@doxwork.com',
    url='https://github.com/mattias-p/digraph123',
    license=license,
    packages=find_packages(exclude=('tests', 'docs'))
)
