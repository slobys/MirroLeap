import {describe,it,expect} from 'vitest';
import {prepareTransferPaths,transferDisplayName,acceptedTransferProgress} from './fileTransfer';
describe('safe file transfer queue',()=>{
    it('deduplicates OS paths without changing order',()=>{
        expect(prepareTransferPaths(['C:\\Users\\a.txt','C:\\Users\\a.txt','/home/b.jpg'])).toEqual({
            files:['C:\\Users\\a.txt','/home/b.jpg']
        });
    });
    it('rejects relative paths, commands and unbounded file batches',()=>{
        expect(prepareTransferPaths(['../passwords.txt']).files).toHaveLength(0);
        expect(prepareTransferPaths(['C:\\a.txt\nother']).files).toHaveLength(0);
        expect(prepareTransferPaths(Array.from({length:31},(_,i)=>'/tmp/'+i)).files).toHaveLength(0);
    });
    it('reports only real, scoped, monotonic ADB transfer progress',()=>{
        const event = {device:'phone-a', transferId:'job_3', percent:47};
        expect(acceptedTransferProgress(event,'job_3','phone-a')).toBe(47);
        expect(acceptedTransferProgress({...event,percent:60},'job_3','phone-a',47)).toBe(60);
        expect(acceptedTransferProgress(event,'job_3','phone-a',47)).toBeNull();
        expect(acceptedTransferProgress({...event,percent:19},'job_3','phone-a',47)).toBeNull();
        expect(acceptedTransferProgress(event,'job_4','phone-a')).toBeNull();
        expect(acceptedTransferProgress(event,'job_3','phone-b')).toBeNull();
        expect(acceptedTransferProgress({...event,percent:101},'job_3','phone-a')).toBeNull();
        expect(acceptedTransferProgress({...event,percent:NaN},'job_3','phone-a')).toBeNull();
        expect(acceptedTransferProgress({...event,percent:12.5},'job_3','phone-a')).toBeNull();
    });
    it('displays the filename rather than the private local path',()=>{
        expect(transferDisplayName('C:\\Secret\\document.pdf')).toBe('document.pdf');
    });
});
