import {describe,it,expect} from 'vitest';
import {prepareTransferPaths,transferDisplayName} from './fileTransfer';
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
    it('displays the filename rather than the private local path',()=>{
        expect(transferDisplayName('C:\\Secret\\document.pdf')).toBe('document.pdf');
    });
});
