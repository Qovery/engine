#!/usr/bin/env python

if __name__ == '__main__':
    with open('.env', 'r') as f:
        envs = [l.strip() for l in f.readlines()]
        print(';'.join(envs))
