module.exports = {
  branches: ['main', , 'feat/*', 'hotfix/*', 'fix/*'],
  plugins: [
    '@semantic-release/commit-analyzer',
    '@semantic-release/release-notes-generator',
    '@semantic-release/changelog',
    '@semantic-release/git',
    '@semantic-release/gitlab',
  ],
};