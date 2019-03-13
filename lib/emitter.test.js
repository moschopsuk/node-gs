const PipeLine = require('./emitter');
let pipeline;

afterEach(() => {
    pipeline.shutdown();
});

describe('pipeline statechange', () => {
    test('pipeline enters Ready state', (done) => {
        pipeline = new PipeLine('videotestsrc ! fakesink');

        pipeline.on('stateChanged', ({ state }) => {
            if (state === 'Ready') {
                done();
            }
        });
    });

    test('pipeline enters Paused state', (done) => {
        pipeline = new PipeLine('videotestsrc ! fakesink');

        pipeline.on('stateChanged', ({ state }) => {
            if (state === 'Paused') {
                done();
            }
        });
    });

    test('pipeline enters Playing state', (done) => {
        pipeline = new PipeLine('videotestsrc ! fakesink');

        pipeline.on('stateChanged', ({ state }) => {
            if (state === 'Playing') {
                done();
            }
        });
    });
});

describe('pipeline parsing', () => {
    test('pipeline calls on error', () => {
        try {
            pipeline = new PipeLine('badger');
        }
        catch(e) {
            expect(e.message).toBe("internal error in Neon module: no element \"badger\"");
        }
    });
});
