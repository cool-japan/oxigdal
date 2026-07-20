module.exports = {
  testEnvironment: 'node',
  testMatch: ['**/__test__/**/*.spec.js'],
  collectCoverage: true,
  coverageDirectory: 'coverage',
  coveragePathIgnorePatterns: ['/node_modules/', '/__test__/'],
  testTimeout: 10000,
  verbose: true
};
