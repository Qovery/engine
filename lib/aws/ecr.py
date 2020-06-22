#!/usr/bin/env python3
import boto3 as boto3

if __name__ == '__main__':
    session = boto3.Session(aws_access_key_id='AKIAZ4KMLSYJLRGNNFNI',
                            aws_secret_access_key='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')

    print(session.client('ec2').describe_instances())


