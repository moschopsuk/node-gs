const logSymbols = require('log-symbols');
const PipeLine = require('./emitter');

const host = "localhost";
const port = 8088;

const pipeline = new PipeLine(`
    videotestsrc is-live=true !
    x264enc !
    mpegtsmux !
    tcpserversink recover-policy=keyframe host=${host} port=${port}
    `);

process.on('SIGINT', () => {
    console.info(logSymbols.warning, 'Shutting down!');
    pipeline.shutdown();
});

pipeline.on('error', ({ message, stack }) => {
    console.error(logSymbols.error, `Error: ${message}`);
    console.error(logSymbols.error, `    Inner message: ${stack}`)
    pipeline.shutdown();
});

pipeline.on('eos', ({}) => {
    console.info(logSymbols.error, 'End of Stream received!');
    pipeline.shutdown();
});

pipeline.on('stateChanged', ({ state }) => {
    console.log(logSymbols.success, `Pipeline changed state to ${state}`);

    if (state == 'Playing') {
        console.info(logSymbols.info, `TCP output created at tcp://${host}:${port}`);
    }
});
