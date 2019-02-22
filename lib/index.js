const PipeLine = require('./emitter');

const host = "localhost";
const port = 8088;

const pipeline = new PipeLine(`
    videotestsrc is-live=true !
    x264enc !
    mpegtsmux !
    tcpserversink recover-policy=keyframe host=${host} port=${port}
    `);

pipeline.on('error', ({ error }) => console.error(error));

console.info(`TCP output created at tcp://${host}:${port}`);
